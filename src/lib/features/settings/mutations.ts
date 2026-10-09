import { createMutation, useQueryClient, type QueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { IgnoreKind } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

/**
 * 保存しないファイルの設定が変わったときに取り直すもの。
 * 変更一覧・すべてのファイルの一覧もパターンの影響を受けるため、プロジェクトの取得も含める。
 */
function invalidateIgnore(client: QueryClient, projectId: string): Promise<unknown> {
	return Promise.all([
		invalidateProject(client, projectId),
		client.invalidateQueries({ queryKey: keys.ignoreRules(projectId) }),
		client.invalidateQueries({ queryKey: keys.trackedIgnored(projectId) })
	]);
}

/**
 * パターンの追加（保存はしない）。追加のあと、保存されているファイルを調べて `onAdded` へ渡す。
 * 該当するファイルがあれば、画面は「保存対象から外す」の案内を出す。
 */
export function useAddIgnorePattern(
	getProjectId: () => string,
	onAdded?: (pattern: string, trackedFiles: string[]) => void
) {
	const client = useQueryClient();
	const retry: { run?: (input: { kind: IgnoreKind; value: string }) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: ({ kind, value }: { kind: IgnoreKind; value: string }) =>
			api.addIgnorePattern(getProjectId(), kind, value),
		onSuccess: async (pattern, { value }) => {
			await invalidateIgnore(client, getProjectId());
			const trackedFiles = await api.listTrackedIgnoredFiles(getProjectId());
			pushToast({ type: 'success', message: t('toast.ignore_added', { name: value }) });
			onAdded?.(pattern, trackedFiles);
		},
		onError: (error, input) => reportError(error, { retry: () => retry.run?.(input) })
	}));
	retry.run = (input) => mutation.mutate(input);
	return mutation;
}

/** パターンの削除（`.gitignore` から 1 行だけ。保存はしない） */
export function useRemoveIgnorePattern(getProjectId: () => string) {
	const client = useQueryClient();
	const retry: { run?: (pattern: string) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (pattern: string) => api.removeIgnorePattern(getProjectId(), pattern),
		onSuccess: async () => {
			await invalidateIgnore(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.ignore_removed') });
		},
		onError: (error, pattern) => reportError(error, { retry: () => retry.run?.(pattern) })
	}));
	retry.run = (pattern) => mutation.mutate(pattern);
	return mutation;
}

/** 追跡中のファイルを保存対象から外して保存する。作業フォルダのファイルは残る */
export function useUntrackFiles(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (paths: string[]) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (paths: string[]) =>
			api.untrackFiles(getProjectId(), t('ignore.untrack_memo'), paths),
		onSuccess: async (_result, paths) => {
			await invalidateIgnore(client, getProjectId());
			pushToast({
				type: 'success',
				message: t('toast.ignore_untracked', { count: paths.length })
			});
			onDone?.();
		},
		onError: (error, paths) => reportError(error, { retry: () => retry.run?.(paths) })
	}));
	retry.run = (paths) => mutation.mutate(paths);
	return mutation;
}
