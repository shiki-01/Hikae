export const ERROR_CODES = [
	'E01',
	'E02',
	'E03',
	'E04',
	'E05',
	'E06',
	'E07',
	'E08',
	'E09',
	'E10',
	'E11',
	'E12',
	'E13',
	'E14',
	'E15',
	'E16',
	'E17',
	'E18',
	'E19',
	'E20'
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export type ErrorParams = Record<string, string | number>;

/** バックエンド（Rust）が返す 3 要素のエラー文言。code と params で画面側の文言を引き、無ければ日本語の 3 要素をそのまま使う */
export interface BackendMessage {
	/** バックエンドのエラーコード（例: `file_in_use`） */
	code: string;
	/** 文言に差し込む値 */
	params: ErrorParams;
	whatHappened: string;
	dataIsSafe: string;
	nextAction: string;
}

export type AppErrorCode = ErrorCode | 'backend';

export class AppError extends Error {
	readonly code: AppErrorCode;
	readonly params: ErrorParams;
	readonly technical: string;
	readonly backend?: BackendMessage;

	constructor(
		code: AppErrorCode,
		technical: string,
		params: ErrorParams = {},
		backend?: BackendMessage
	) {
		super(`${code}: ${technical}`);
		this.name = 'AppError';
		this.code = code;
		this.params = params;
		this.technical = technical;
		this.backend = backend;
	}
}

/** バックエンドのエラーコードのうち、設計 5章の E03 / E04 に当たるもの（通信断、GitHub 側の障害） */
const BACKEND_AUTO_RECOVERING: Record<string, ErrorCode> = {
	network_unavailable: 'E03',
	github_unavailable: 'E04'
};

/**
 * バックエンドのエラーコードに対応する、自動で回復する画面側のコード（E03 / E04）。
 * 当てはまらなければ undefined
 */
export function autoRecoveringScreenCode(backendCode: string): ErrorCode | undefined {
	return Object.hasOwn(BACKEND_AUTO_RECOVERING, backendCode)
		? BACKEND_AUTO_RECOVERING[backendCode]
		: undefined;
}

/** 自動で回復するエラーか（画面側のコード E03 / E04 と、それに当たるバックエンドのコード） */
export function isAutoRecovering(code: string): boolean {
	return code === 'E03' || code === 'E04' || autoRecoveringScreenCode(code) !== undefined;
}
