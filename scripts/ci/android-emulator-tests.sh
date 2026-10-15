#!/usr/bin/env bash
# エミュレータ上で行うテストの入口（android-emulator-runner から呼ばれる）。
# 1. Keystore を使う SecretBox の端末上のテスト（プラグインの androidTest）
# 2. APK を入れて起動する確認（android-smoke.sh）
set -euo pipefail

APK=$(find src-tauri/mobile/gen/android/app/build/outputs/apk -name '*.apk' | head -n 1)
test -n "$APK" || { echo "::error::APK が見つかりません"; exit 1; }
echo "APK: $APK"

# 先にスモークテスト（アプリの起動と通信）を行い、失敗してもテストの結果は見られるよう、両方を実行して最後にまとめる
status=0
bash scripts/ci/android-smoke.sh "$APK" smoke-out || status=1

echo "--- SecretBox の端末上のテスト（Android Keystore） ---"
(cd src-tauri/mobile/gen/android && ./gradlew --no-daemon :tauri-plugin-hikae-secure-store:connectedDebugAndroidTest) || status=1

exit "$status"
