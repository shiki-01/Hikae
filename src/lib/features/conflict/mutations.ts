import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { ConflictResolution } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

export function useResolve(getProjectId: () => string, onResolved?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (resolutions: ConflictResolution[]) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (resolutions: ConflictResolution[]) =>
			api.resolveConflicts(getProjectId(), resolutions),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.resolved') });
			onResolved?.();
		},
		onError: (error, resolutions) => reportError(error, { retry: () => retry.run?.(resolutions) })
	}));
	retry.run = (resolutions) => mutation.mutate(resolutions);
	return mutation;
}

export function useAbortMerge(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: () => api.abortMerge(getProjectId()),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'info', message: t('toast.merge_aborted') });
			onDone?.();
		},
		onError: (error) => reportError(error, { retry: () => retry.run() })
	}));
	retry.run = () => mutation.mutate();
	return mutation;
}
