import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { RestoreScope } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

interface RestoreVariables {
	targetId: string;
	scope: RestoreScope;
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
			api.restore(getProjectId(), variables.targetId, variables.scope),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			pushToast({
				type: 'success',
				message: t('toast.restored', { count: result.changedCount }),
				actionLabel: t('toast.undo'),
				onaction: () => undo.mutate(result.undoToken)
			});
			onRestored?.();
		},
		onError: (error, variables) => reportError(error, { retry: () => retry.run?.(variables) })
	}));
	retry.run = (variables) => mutation.mutate(variables);
	return mutation;
}
