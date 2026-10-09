import { hasMessage, t, type MessageKey } from '#lib/i18n/index.js';
import {
	AppError,
	autoRecoveringScreenCode,
	isAutoRecovering,
	type AppErrorCode,
	type BackendMessage,
	type ErrorCode
} from '#lib/api/errors.js';

/**
 * 次の行動ボタンの種類。
 * - `retry`: 元の操作をもう一度実行する（実行できる操作のときだけ）
 * - `login`: ログインし直す画面へ移る
 * - `copy`: 管理者向けの説明をクリップボードへコピーする
 * - `guide`: 対応する画面へ誘導する（呼び出し側が行き先を渡したときだけ）
 * - `close`: 閉じるだけ
 */
export type PrimaryKind = 'retry' | 'login' | 'copy' | 'guide' | 'close';

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

/** 呼び出し側が用意できる行動。用意できない行動のボタンは「閉じる」だけにする */
export interface AvailableActions {
	/** 元の操作を再実行できる */
	retry: boolean;
	/** 対応する画面へ誘導できる */
	guide: boolean;
}

const ALL_ACTIONS: AvailableActions = { retry: true, guide: true };

// 画面側のコード（設計 5章 E01〜E20）
const RETRY_CODES: ErrorCode[] = ['E04', 'E12'];
const COPY_CODES: ErrorCode[] = ['E02'];
const LOGIN_CODES: ErrorCode[] = ['E01'];
const SECONDARY_CODES: ErrorCode[] = ['E05', 'E11', 'E15'];

/**
 * バックエンドのエラーコードごとの次の行動ボタン（設計 5.1）。
 * ここに無いコードは「閉じる」だけ。コードを足すときは、errcode.<code>.* の文言も用意する
 */
const BACKEND_ACTIONS: Record<string, Exclude<PrimaryKind, 'close'>> = {
	// 認証が切れた（E01）: ログインし直す
	not_logged_in: 'login',
	// 権限が足りない（E02）: 管理者向けの説明をコピーする
	github_forbidden: 'copy',
	// 一時的な失敗: 元の操作をもう一度行う
	file_in_use: 'retry',
	git_failed: 'retry',
	git_timeout: 'retry',
	io_error: 'retry',
	safety_check_failed: 'retry',
	github_unavailable: 'retry',
	github_rate_limited: 'retry',
	github_error: 'retry',
	clone_failed: 'retry',
	clone_timeout: 'retry',
	trash_failed: 'retry',
	discard_not_backed_up: 'retry',
	// 保存先の設定に関わるもの: 保存先の画面へ誘導する
	remote_name_taken: 'guide',
	remote_name_invalid: 'guide',
	remote_owner_invalid: 'guide',
	remote_public_not_confirmed: 'guide',
	remote_conflict: 'guide',
	remote_orphaned: 'guide'
};

/** バックエンドのエラーコードに対する次の行動の種類。対応表に無ければ close */
export function primaryKindOfBackendCode(code: string): PrimaryKind {
	return Object.hasOwn(BACKEND_ACTIONS, code) ? BACKEND_ACTIONS[code] : 'close';
}

const KIND_LABELS: Record<Exclude<PrimaryKind, 'close'>, MessageKey> = {
	retry: 'errbtn.retry',
	login: 'errbtn.login',
	copy: 'errbtn.copy_admin',
	guide: 'errbtn.guide_remote'
};

/** 文言の差し込み位置が残っている（値が足りなかった）か */
function hasUnresolved(text: string): boolean {
	return /\{\w+\}/.test(text);
}

/**
 * バックエンドのエラーコードから、画面側の 3 要素の文言を引く。
 * コードが未知・文言が無い・差し込む値が足りないときは、バックエンドが返した日本語をそのまま使う。
 */
export function resolveBackendMessage(message: BackendMessage): BackendMessage {
	const base = `errcode.${message.code}`;
	const keys = [`${base}.what`, `${base}.safe`, `${base}.next`];
	if (!keys.every(hasMessage)) return message;
	const [whatHappened, dataIsSafe, nextAction] = keys.map((key) =>
		t(key as MessageKey, message.params)
	);
	if ([whatHappened, dataIsSafe, nextAction].some(hasUnresolved)) return message;
	return { ...message, whatHappened, dataIsSafe, nextAction };
}

/** 用意できない行動のボタンは、閉じるだけにする */
function withAvailable(kind: PrimaryKind, available: AvailableActions): PrimaryKind {
	if (kind === 'retry' && !available.retry) return 'close';
	if (kind === 'guide' && !available.guide) return 'close';
	return kind;
}

function describeBackend(
	message: BackendMessage,
	technical: string,
	available: AvailableActions
): ErrorView {
	const { whatHappened, dataIsSafe, nextAction } = resolveBackendMessage(message);
	const kind = withAvailable(primaryKindOfBackendCode(message.code), available);
	// 自動で回復するもの（通信断など）は、ダイアログではなく控えめなトーストで知らせる
	const autoCode = autoRecoveringScreenCode(message.code);
	return {
		code: 'backend',
		title: whatHappened,
		message: [dataIsSafe, nextAction].filter((line) => line.length > 0).join('\n'),
		primaryLabel:
			kind === 'close'
				? t('error.close')
				: autoCode === 'E04'
					? t('error.E04.button')
					: t(KIND_LABELS[kind]),
		primaryKind: kind,
		toastMessage: autoCode ? t(`error.${autoCode}.toast` as MessageKey) : undefined,
		copyText: kind === 'copy' ? t('error.E02.copy_text') : undefined,
		technical,
		autoRecovering: autoCode !== undefined
	};
}

export function describeError(
	error: unknown,
	available: AvailableActions = ALL_ACTIONS
): ErrorView {
	if (error instanceof AppError && error.code === 'backend' && error.backend) {
		return describeBackend(error.backend, error.technical, available);
	}
	if (error instanceof AppError && error.code !== 'backend') {
		const code = error.code;
		const kind: PrimaryKind = RETRY_CODES.includes(code)
			? 'retry'
			: COPY_CODES.includes(code)
				? 'copy'
				: LOGIN_CODES.includes(code)
					? 'login'
					: 'close';
		const primaryKind = withAvailable(kind, available);
		const params = error.params;
		const secondaryKey = `error.${code}.button2` as MessageKey;
		return {
			code,
			title: t(`error.${code}.title` as MessageKey, params),
			message: t(`error.${code}.message` as MessageKey, params),
			primaryLabel:
				primaryKind === kind ? t(`error.${code}.button` as MessageKey, params) : t('error.close'),
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
