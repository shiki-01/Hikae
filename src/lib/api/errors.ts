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

/** バックエンド（Rust）が返す 3 要素のエラー文言 */
export interface BackendMessage {
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

export function isAutoRecovering(code: AppErrorCode): boolean {
	return code === 'E03' || code === 'E04';
}
