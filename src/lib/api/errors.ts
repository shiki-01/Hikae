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

export class AppError extends Error {
	readonly code: ErrorCode;
	readonly params: ErrorParams;
	readonly technical: string;

	constructor(code: ErrorCode, technical: string, params: ErrorParams = {}) {
		super(`${code}: ${technical}`);
		this.name = 'AppError';
		this.code = code;
		this.params = params;
		this.technical = technical;
	}
}

export function isAutoRecovering(code: ErrorCode): boolean {
	return code === 'E03' || code === 'E04';
}
