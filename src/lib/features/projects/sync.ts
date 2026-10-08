import { createMutation, useQueryClient, type QueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import type { FetchOptions } from '#lib/api/types.js';
import { syncSizeRequest, type SyncSizeRequest } from './sync-size';

export function invalidateProject(client: QueryClient, projectId: string): Promise<unknown> {
	return Promise.all([
		client.invalidateQueries({ queryKey: keys.projects }),
		client.invalidateQueries({ queryKey: keys.project(projectId) }),
		client.invalidateQueries({ queryKey: keys.changes(projectId) }),
		client.invalidateQueries({ queryKey: keys.projectTree(projectId) }),
		client.invalidateQueries({ queryKey: keys.history(projectId) }),
		client.invalidateQueries({ queryKey: keys.conflicts(projectId) }),
		client.invalidateQueries({ queryKey: ['compare', projectId] }),
		client.invalidateQueries({ queryKey: ['impact', projectId] }),
		client.invalidateQueries({ queryKey: ['point-files', projectId] }),
		client.invalidateQueries({ queryKey: keys.memoSuggestion(projectId) })
	]);
}

/** 取り込みの実行時の指定。画面から呼ぶときは省略する。確認で承諾した後だけ `saveConfirmed` を渡す */
export type FetchVariables = FetchOptions | void;

export function useFetch(
	getProjectId: () => string,
	onConflict?: () => void,
	onSizeCheck?: (request: SyncSizeRequest) => void,
	onSaveConfirm?: (unsavedCount: number) => void
) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: (variables: FetchVariables) =>
			api.fetch(getProjectId(), variables ? variables : undefined),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			const skipped = syncSizeRequest('fetch', result);
			if (result.unsavedCount !== null) {
				// 未保存の変更があり、保存の確認が必要なため何も実行していない。成功のトーストは出さない
				onSaveConfirm?.(result.unsavedCount);
			} else if (skipped) {
				// 大きいファイルのため何も実行していない。成功のトーストは出さない
				onSizeCheck?.(skipped);
			} else if (result.conflictCount > 0) {
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

export function usePush(
	getProjectId: () => string,
	onSizeCheck?: (request: SyncSizeRequest) => void
) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: () => api.push(getProjectId()),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			const skipped = syncSizeRequest('push', result);
			if (skipped) onSizeCheck?.(skipped);
			else pushToast({ type: 'success', message: t('toast.pushed') });
		},
		onError: (error) => reportError(error, { retry: () => retry.run() })
	}));
	retry.run = () => mutation.mutate();
	return mutation;
}
