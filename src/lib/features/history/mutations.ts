import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { AfterRestoreSave, RestoreScope } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

interface RestoreVariables {
	targetId: string;
	scope: RestoreScope;
	/** 元に戻した後に続けて保存するときのメモ（設定がオンのときだけバックエンドが使う） */
	saveMemo?: string;
}

/** 戻した後の保存の結果に合わせた、完了の通知 */
function restoredToast(save: AfterRestoreSave, count: number) {
	switch (save) {
		case 'saved':
			return { type: 'success' as const, message: t('toast.restored_saved', { count }) };
		case 'needs_size_decision':
			return { type: 'warning' as const, message: t('toast.restored_save_large', { count }) };
		case 'failed':
			return { type: 'warning' as const, message: t('toast.restored_save_failed', { count }) };
		default:
			return { type: 'success' as const, message: t('toast.restored', { count }) };
	}
}

export function useRestore(getProjectId: () => string, onRestored?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (variables: RestoreVariables) => void } = {};

	const undo = createMutation(() => ({
		mutationFn: (undoToken: string) => api.undoRestore(getProjectId(), undoToken),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'info', message: t('toast.undone') });
		},
		onError: (error) => reportError(error)
	}));

	const mutation = createMutation(() => ({
		mutationFn: (variables: RestoreVariables) =>
			api.restore(getProjectId(), variables.targetId, variables.scope, variables.saveMemo),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			// 取り消し用の復元点が無い（何も変えていない）ときは、取り消しの操作を出さない
			const undoToken = result.undoToken;
			pushToast({
				...restoredToast(result.save, result.changedCount),
				...(undoToken
					? { actionLabel: t('toast.undo'), onaction: () => undo.mutate(undoToken) }
					: {})
			});
			onRestored?.();
		},
		onError: (error, variables) => reportError(error, { retry: () => retry.run?.(variables) })
	}));
	retry.run = (variables) => mutation.mutate(variables);
	return mutation;
}
