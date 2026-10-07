import { t, type MessageKey } from '#lib/i18n/index.js';
import { AppError, isAutoRecovering, type AppErrorCode, type ErrorCode } from '#lib/api/errors.js';

export type PrimaryKind = 'retry' | 'copy' | 'close';

export interface ErrorView {
	code: AppErrorCode | 'generic';
	title: string;
	message: string;
	primaryLabel: string;
	primaryKind: PrimaryKind;
	secondaryLabel?: string;
	toastMessage?: string;
	copyText?: string;
	technical: string;
	autoRecovering: boolean;
}

const RETRY_CODES: ErrorCode[] = ['E04', 'E12'];
const COPY_CODES: ErrorCode[] = ['E02'];
const SECONDARY_CODES: ErrorCode[] = ['E05', 'E11', 'E15'];

export function describeError(error: unknown): ErrorView {
	if (error instanceof AppError && error.code === 'backend' && error.backend) {
		const { whatHappened, dataIsSafe, nextAction } = error.backend;
		return {
			code: 'backend',
			title: whatHappened,
			message: [dataIsSafe, nextAction].filter((line) => line.length > 0).join('\n'),
			primaryLabel: t('error.close'),
			primaryKind: 'close',
			technical: error.technical,
			autoRecovering: false
		};
	}
	if (error instanceof AppError && error.code !== 'backend') {
		const code = error.code;
		const primaryKind: PrimaryKind = RETRY_CODES.includes(code)
			? 'retry'
			: COPY_CODES.includes(code)
				? 'copy'
				: 'close';
		const params = error.params;
		const secondaryKey = `error.${code}.button2` as MessageKey;
		return {
			code,
			title: t(`error.${code}.title` as MessageKey, params),
			message: t(`error.${code}.message` as MessageKey, params),
			primaryLabel: t(`error.${code}.button` as MessageKey, params),
			primaryKind,
			secondaryLabel: SECONDARY_CODES.includes(code) ? t(secondaryKey, params) : undefined,
			toastMessage: isAutoRecovering(code) ? t(`error.${code}.toast` as MessageKey) : undefined,
			copyText: code === 'E02' ? t('error.E02.copy_text') : undefined,
			technical: error.technical,
			autoRecovering: isAutoRecovering(code)
		};
	}
	return {
		code: 'generic',
		title: t('error.generic.title'),
		message: t('error.generic.message'),
		primaryLabel: t('error.close'),
		primaryKind: 'close',
		technical: error instanceof Error ? error.message : String(error),
		autoRecovering: false
	};
}
