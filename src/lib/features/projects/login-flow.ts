import { AppError } from '#lib/api/errors.js';
import type { LoginOutcome } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

/** ログインの失敗を、画面で 3 要素（何が起きたか・データは無事か・次の行動）に分けて見せる種類 */
export type LoginFailureKind = 'denied' | 'expired' | 'setup';

export interface LoginFailureView {
	kind: LoginFailureKind;
	/** 何が起きたか */
	title: MessageKey;
	/** データは無事か */
	dataIsSafe: MessageKey;
	/** 次の行動 */
	nextAction: MessageKey;
	/** 「もう一度ログイン」を出すか。設定が無い場合は何度試しても同じなので出さない */
	retryable: boolean;
}

/** バックエンドが技術情報に入れる、ログインの設定が無いことを示す値 */
const SETUP_TECHNICAL = ['MissingClientId', 'DeviceFlowDisabled'];

/** クライアント ID が未設定など、アプリ側の設定が必要なために始められないエラーか */
export function isLoginSetupError(error: unknown): boolean {
	return (
		error instanceof AppError &&
		error.code === 'backend' &&
		SETUP_TECHNICAL.some((value) => error.technical === value)
	);
}

const FAILURES: Record<LoginFailureKind, LoginFailureView> = {
	expired: {
		kind: 'expired',
		title: 'login.error.expired.title',
		dataIsSafe: 'login.error.data_is_safe',
		nextAction: 'login.error.expired.next',
		retryable: true
	},
	denied: {
		kind: 'denied',
		title: 'login.error.denied.title',
		dataIsSafe: 'login.error.data_is_safe',
		nextAction: 'login.error.denied.next',
		retryable: true
	},
	setup: {
		kind: 'setup',
		title: 'login.error.setup.title',
		dataIsSafe: 'login.error.data_is_safe',
		nextAction: 'login.error.setup.next',
		retryable: false
	}
};

export function setupFailure(): LoginFailureView {
	return FAILURES.setup;
}

/** ログインの結末に応じた失敗の表示。成功・キャンセルは失敗ではないので null */
export function failureForOutcome(outcome: LoginOutcome): LoginFailureView | null {
	if (outcome === 'expired') return FAILURES.expired;
	if (outcome === 'denied') return FAILURES.denied;
	return null;
}

/** 残り秒数を「分:秒」にする。負の値は 0:00 */
export function formatCountdown(seconds: number): string {
	const total = Math.max(0, Math.floor(seconds));
	const minutes = Math.floor(total / 60);
	const rest = String(total % 60).padStart(2, '0');
	return `${minutes}:${rest}`;
}

/** 外部のページへのリンクとして安全に使える（https の）URL か */
export function isHttpsUrl(value: string): boolean {
	try {
		return new URL(value).protocol === 'https:';
	} catch {
		return false;
	}
}
