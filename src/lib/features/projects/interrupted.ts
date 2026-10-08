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
