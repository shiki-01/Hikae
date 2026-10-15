#!/usr/bin/env bash
# iOS シミュレータで .app を起動して確かめる（.github/workflows/mobile.yml の ios ジョブ）。
# アプリが起動して、一定時間落ちないこと。画面のスクリーンショットを残す。
set -euo pipefail

APP="${1:?.app のパス}"
OUT="${2:-smoke-out}"
BUNDLE_ID=com.shiki01.hikae.mobile
mkdir -p "$OUT"

# 使えるシミュレータ（iPhone）を 1 つ選ぶ
UDID=$(xcrun simctl list devices available | grep -E "iPhone" | head -n 1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')
[ -n "$UDID" ] || { echo "::error::iPhone のシミュレータが見つかりません"; exit 1; }
echo "シミュレータ: $UDID"

# 失敗したときに、原因が job のログだけで分かるよう、アプリのログを出す（artifact は取得できない環境もあるため）
fail() {
	echo "::error::$1"
	xcrun simctl spawn "$UDID" log show --last 3m --style compact \
		--predicate 'process == "hikae-mobile" OR process == "Hikae"' 2>&1 | tail -n 100 || true
	exit 1
}

xcrun simctl boot "$UDID" || true
xcrun simctl bootstatus "$UDID" -b
xcrun simctl install "$UDID" "$APP"
xcrun simctl launch "$UDID" "$BUNDLE_ID"

# 起動直後・20 秒後・40 秒後に生きているか確かめる
sleep 20
xcrun simctl io "$UDID" screenshot "$OUT/01-start.png"
running() { xcrun simctl spawn "$UDID" launchctl list | grep -q "UIKitApplication:$BUNDLE_ID"; }
running || fail "アプリが終了しています（20 秒後）"
sleep 20
xcrun simctl io "$UDID" screenshot "$OUT/02-later.png"
running || fail "アプリが終了しています（40 秒後）"

# 診断用: アプリのログ
xcrun simctl spawn "$UDID" log show --last 2m --style compact \
	--predicate 'process == "hikae-mobile" OR process == "Hikae"' > "$OUT/app.log" 2>&1 || true
if ls ~/Library/Logs/DiagnosticReports/*ikae* > /dev/null 2>&1; then
	echo "::error::クラッシュレポートがあります"
	cp ~/Library/Logs/DiagnosticReports/*ikae* "$OUT/" || true
	exit 1
fi
echo "--- アプリのログ（末尾） ---"
tail -n 40 "$OUT/app.log" || true
echo "OK"
