// モバイル版（src-tauri/mobile）の Tauri CLI を実行する。
// 使い方: pnpm tauri:mobile ios init / ios dev / android init / android dev など
// TAURI_APP_PATH で、デスクトップ版（src-tauri/app）ではなくモバイル版の設定を指す（設計書 14.3）。
// 環境変数の設定方法が OS ごとに違うため、Node から起動する
import { spawnSync } from 'node:child_process';

const result = spawnSync('pnpm', ['exec', 'tauri', ...process.argv.slice(2)], {
	stdio: 'inherit',
	shell: process.platform === 'win32',
	env: { ...process.env, TAURI_APP_PATH: 'src-tauri/mobile' }
});
process.exit(result.status ?? 1);
