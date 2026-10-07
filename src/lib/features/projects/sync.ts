import { createMutation, useQueryClient, type QueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';

export function invalidateProject(client: QueryClient, projectId: string): Promise<unknown> {
	return Promise.all([
		client.invalidateQueries({ queryKey: keys.projects }),
		client.invalidateQueries({ queryKey: keys.project(projectId) }),
		client.invalidateQueries({ queryKey: keys.changes(projectId) }),
		client.invalidateQueries({ queryKey: keys.history(projectId) }),
		client.invalidateQueries({ queryKey: keys.conflicts(projectId) }),
		client.invalidateQueries({ queryKey: ['compare', projectId] }),
		client.invalidateQueries({ queryKey: ['impact', projectId] }),
		client.invalidateQueries({ queryKey: ['point-files', projectId] }),
		client.invalidateQueries({ queryKey: keys.memoSuggestion(projectId) })
	]);
}

export function useFetch(getProjectId: () => string, onConflict?: () => void) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: () => api.fetch(getProjectId()),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			if (result.conflictCount > 0) {
				onConflict?.();
			} else if (result.mergedCount > 0) {
				pushToast({ type: 'success', message: t('toast.fetched', { count: result.mergedCount }) });
			} else {
				pushToast({ type: 'info', message: t('toast.fetch_none') });
			}
		},
		onError: (error) => reportError(error, { retry: () => retry.run() })
	}));
	retry.run = () => mutation.mutate();
	return mutation;
}

export function usePush(getProjectId: () => string) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: () => api.push(getProjectId()),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.pushed') });
		},
		onError: (error) => reportError(error, { retry: () => retry.run() })
	}));
	retry.run = () => mutation.mutate();
	return mutation;
}
