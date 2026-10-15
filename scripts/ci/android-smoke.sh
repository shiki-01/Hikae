#!/usr/bin/env bash
# エミュレータ上で APK を起動して確かめる（.github/workflows/mobile.yml の android-smoke）。
# 1. アプリが起動し、落ちない  2. ログイン画面が出る
# 3. 「GitHub でログイン」を押すと、Rust が GitHub に HTTPS で問い合わせ、確認コードが出る
#    （Android の証明書検証・通信・Device Flow の設定が、実機相当で動くことの確認）
set -euo pipefail

APK="${1:?APK のパス}"
OUT="${2:-smoke-out}"
PKG=com.shiki01.hikae.mobile
mkdir -p "$OUT"

# 失敗したときに、原因が job のログだけで分かるよう、画面の文字とログの抜粋を出す
# （artifact は取得できない環境もあるため）
diagnose() {
	adb logcat -d > "$OUT/logcat.txt" || true
	adb exec-out screencap -p > "$OUT/failure.png" || true
	dump_ui failure
	echo "::group::画面の文字（uiautomator）"
	grep -ohE '(text|content-desc)="[^"]+"' "$OUT"/failure.xml 2>/dev/null | sort -u | head -60 || true
	echo "::endgroup::"
	echo "::group::プロセス"
	adb shell pidof "$PKG" || echo "（プロセスなし）"
	echo "::endgroup::"
	echo "::group::logcat（アプリ・WebView・Rust の抜粋）"
	# uiautomator 自身のログ（AndroidRuntime）は除く
	grep -iE "chromium|tauri|RustStdoutStderr|hikae|WebView|console|panicked|FATAL EXCEPTION|reqwest|rustls" "$OUT/logcat.txt" \
		| grep -v "AndroidRuntime: \(>>>>>>\|Using default\|Leaving\|Calling main\|Shutting\)" | cut -c1-300 | tail -n 150 || true
	echo "::endgroup::"
}

fail() {
	echo "::error::$1"
	diagnose
	exit 1
}

dump_ui() {
	adb shell uiautomator dump /sdcard/ui.xml > /dev/null 2>&1 || true
	adb pull /sdcard/ui.xml "$OUT/$1.xml" > /dev/null 2>&1 || true
}

adb install -r "$APK"
adb logcat -c
adb shell am start -W -n "$PKG/.MainActivity"

# 画面が出るまで待つ（最大 60 秒）
found=""
for _ in $(seq 1 30); do
	sleep 2
	dump_ui 01-start
	if node scripts/ci/uiautomator.mjs center "$OUT/01-start.xml" "GitHub でログイン" > "$OUT/login-button.txt" 2>/dev/null; then
		found=1
		break
	fi
done
adb exec-out screencap -p > "$OUT/01-start.png"
[ -n "$found" ] || fail "ログイン画面（『GitHub でログイン』）が表示されませんでした"
adb shell pidof "$PKG" > /dev/null || fail "アプリが終了しています"

# ボタンを押して、確認コードが出るまで待つ（最大 60 秒）
read -r X Y < "$OUT/login-button.txt"
adb shell input tap "$X" "$Y"
code=""
for _ in $(seq 1 30); do
	sleep 2
	dump_ui 02-code
	echo "画面: $(grep -ohE 'text="[^"]+"' "$OUT/02-code.xml" 2>/dev/null | tr '\n' ' ' | cut -c1-300)"
	if code=$(node scripts/ci/uiautomator.mjs match "$OUT/02-code.xml" '[A-Z0-9]{4}-[A-Z0-9]{4}' 2>/dev/null); then
		break
	fi
	code=""
done
adb exec-out screencap -p > "$OUT/02-code.png"
[ -n "$code" ] || fail "確認コードが表示されませんでした（Rust から GitHub への通信が失敗した可能性があります）"
echo "確認コードを取得しました: $code"

# 落ちていないこと
adb shell pidof "$PKG" > /dev/null || fail "アプリが終了しています（確認コードの表示後）"
adb logcat -d > "$OUT/logcat.txt"
if grep -E "FATAL EXCEPTION|panicked at" "$OUT/logcat.txt"; then
	fail "ログに致命的なエラーがあります"
fi
echo "OK"
