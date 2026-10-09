import type { ToastType } from '#lib/components/Toast.svelte';
import { describeError, type ErrorView } from './error-view';

export interface ToastItem {
	id: number;
	type: ToastType;
	message: string;
	actionLabel?: string;
	onaction?: () => void;
}

export interface ErrorState {
	view: ErrorView;
	onretry?: () => void;
	onsecondary?: () => void;
	onprimary?: () => void;
}

interface Store {
	toasts: ToastItem[];
	error: ErrorState | null;
}

export const notifications: Store = $state({ toasts: [], error: null });

const AUTO_DISMISS_MS: Record<ToastType, number | null> = {
	info: 5000,
	success: 5000,
	warning: 8000,
	error: null
};

let sequence = 0;

export function dismissToast(id: number): void {
	notifications.toasts = notifications.toasts.filter((toast) => toast.id !== id);
}

export function pushToast(input: Omit<ToastItem, 'id'>): number {
	sequence += 1;
	const id = sequence;
	notifications.toasts = [...notifications.toasts, { ...input, id }];
	const timeout = AUTO_DISMISS_MS[input.type];
	if (timeout !== null) setTimeout(() => dismissToast(id), timeout);
	return id;
}

export interface ReportOptions {
	/** 元の操作をもう一度実行する。渡したときだけ、「もう一度」のボタンを出す */
	retry?: () => void;
	onsecondary?: () => void;
	/** 主ボタンを押したときの動作。保存先の画面などへ誘導するエラーでは、これを渡したときだけ誘導のボタンを出す */
	onprimary?: () => void;
}

export function reportError(error: unknown, options: ReportOptions = {}): void {
	const view = describeError(error, {
		retry: options.retry !== undefined,
		guide: options.onprimary !== undefined
	});
	if (view.autoRecovering) {
		pushToast({
			type: 'warning',
			message: view.toastMessage ?? view.title,
			actionLabel: view.primaryKind === 'retry' ? view.primaryLabel : undefined,
			onaction: options.retry
		});
		return;
	}
	notifications.error = {
		view,
		onretry: options.retry,
		onsecondary: options.onsecondary,
		onprimary: options.onprimary
	};
}

export function closeError(): void {
	notifications.error = null;
}
