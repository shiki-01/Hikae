import { t } from '#lib/i18n/index.js';

/** 途中で止まった操作の名前（バックエンドの操作名）を画面用の言葉にする */
export function interruptedOperationName(operation: string | null): string {
	switch (operation) {
		case 'save':
			return t('interrupted.op_save');
		case 'pull':
			return t('interrupted.op_pull');
		case 'push':
			return t('interrupted.op_push');
		case 'restore':
			return t('interrupted.op_restore');
		case 'resolve':
			return t('interrupted.op_resolve');
		default:
			return t('interrupted.op_other');
	}
}

/** 中断ダイアログの処理の状態。いずれかの処理の実行中は、ほかの処理を受け付けない */
export type InterruptedPhase = 'idle' | 'recovering' | 'dismissing';

/** 中断ダイアログで利用者が選んだこと */
export type InterruptedAction = 'recover' | 'dismiss' | 'close';

export interface InterruptedStep {
	/** 次の処理の状態 */
	phase: InterruptedPhase;
	/** 今から実行する処理（無ければ null） */
	run: 'recover' | 'dismiss' | null;
	/** ダイアログを閉じるか */
	close: boolean;
}

/**
 * 中断ダイアログの状態遷移（純関数）。
 * - 待機中: 「操作前の状態に戻す」「このまま続ける」は、それぞれの処理を始める。「閉じる」はダイアログを閉じる
 * - 処理中: どの操作も受け付けない（ダイアログの閉じるも無効）。処理の終わりは呼び出し側が結果に合わせて閉じる
 */
export function interruptedDialogStep(
	phase: InterruptedPhase,
	action: InterruptedAction
): InterruptedStep {
	if (phase !== 'idle') return { phase, run: null, close: false };
	switch (action) {
		case 'recover':
			return { phase: 'recovering', run: 'recover', close: false };
		case 'dismiss':
			return { phase: 'dismissing', run: 'dismiss', close: false };
		case 'close':
			return { phase: 'idle', run: null, close: true };
	}
}
