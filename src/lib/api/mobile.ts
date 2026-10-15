// モバイル版の API の実装（Tauri のコマンド呼び出し）。バインディングは src/lib/bindings-mobile.ts
// （`cargo run -p hikae-mobile --bin export-bindings-mobile` で生成。手で編集しない）。
// トークンは Rust 側だけが持つ。ここには渡ってこない。

import { commands } from '#lib/bindings-mobile.js';
import { AppError } from './errors';
import type { MobileApi } from './mobile-types';

type MobileBackendError = {
	code: string;
	params: [string, string][];
	whatHappened: string;
	dataIsSafe: string;
	nextAction: string;
	technicalInfo: string | null;
};

type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: MobileBackendError };

/** バックエンドのエラー（3 要素）を画面側の AppError に変換する（mapError と同じ形） */
export function mapMobileError(error: MobileBackendError): AppError {
	return new AppError(
		'backend',
		error.technicalInfo ?? '',
		{},
		{
			code: error.code,
			params: Object.fromEntries(error.params),
			whatHappened: error.whatHappened,
			dataIsSafe: error.dataIsSafe,
			nextAction: error.nextAction
		}
	);
}

/** Result 形式の戻り値を展開し、エラーは AppError として throw する */
async function unwrap<T>(promise: Promise<Result<T>>): Promise<T> {
	const result = await promise;
	if (result.status === 'error') throw mapMobileError(result.error);
	return result.data;
}

export const mobileTauriApi: MobileApi = {
	getAccount: () => unwrap(commands.mobileAccount()),
	async startLogin() {
		const start = await unwrap(commands.mobileStartLogin());
		return {
			userCode: start.userCode,
			verificationUri: start.verificationUri,
			expiresInSecs: start.expiresIn
		};
	},
	async openLoginPage() {
		await unwrap(commands.mobileOpenLoginPage());
	},
	waitLogin: () => unwrap(commands.mobileWaitLogin()),
	async cancelLogin() {
		await commands.mobileCancelLogin();
	},
	async logout() {
		await unwrap(commands.mobileLogout());
	},
	listRepos: () => unwrap(commands.mobileListRepos()),
	listDir: (owner, name, path, ref) =>
		unwrap(commands.mobileListDir(owner, name, path, ref ?? null)),
	preview: (owner, name, path, sha, ref) =>
		unwrap(commands.mobilePreview(owner, name, path, sha, ref ?? null)),
	download: (owner, name, path, sha, ref) =>
		unwrap(commands.mobileDownload(owner, name, path, sha, ref ?? null)),
	fileHistory: (owner, name, path) => unwrap(commands.mobileFileHistory(owner, name, path)),
	planUpload: (owner, name, items) => unwrap(commands.mobilePlanUpload(owner, name, items)),
	upload: (owner, name, message, items) =>
		unwrap(commands.mobileUpload(owner, name, message, items))
};
