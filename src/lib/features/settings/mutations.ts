import { createMutation, useQueryClient, type QueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { IgnoreKind } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';
import { baseName } from './skip-stash-model';

/**
 * 保存しないファイルの設定が変わったときに取り直すもの。
 * 変更一覧・すべてのファイルの一覧もパターンの影響を受けるため、プロジェクトの取得も含める。
 */
function invalidateIgnore(client: QueryClient, projectId: string): Promise<unknown> {
	return Promise.all([
		invalidateProject(client, projectId),
		client.invalidateQueries({ queryKey: keys.ignoreRules(projectId) }),
		client.invalidateQueries({ queryKey: keys.trackedIgnored(projectId) }),
		client.invalidateQueries({ queryKey: keys.skipWorktree(projectId) }),
		client.invalidateQueries({ queryKey: keys.skipDrift(projectId) }),
		client.invalidateQueries({ queryKey: keys.skipStash(projectId) })
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

/** 今後の変更だけ無視する（追跡中のファイルだけ。作業フォルダのファイルは変えない。設計書 4.8） */
export function useSetSkipWorktree(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (paths: string[]) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (paths: string[]) => api.setSkipWorktree(getProjectId(), paths),
		onSuccess: async (_result, paths) => {
			await invalidateIgnore(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.ignore_skipped', { count: paths.length }) });
			onDone?.();
		},
		onError: (error, paths) => reportError(error, { retry: () => retry.run?.(paths) })
	}));
	retry.run = (paths) => mutation.mutate(paths);
	return mutation;
}

/** 無視をやめる（手元の変更は通常の未保存の変更として現れる） */
export function useClearSkipWorktree(getProjectId: () => string) {
	const client = useQueryClient();
	const retry: { run?: (paths: string[]) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (paths: string[]) => api.clearSkipWorktree(getProjectId(), paths),
		onSuccess: async (_result, paths) => {
			await invalidateIgnore(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.ignore_cleared', { count: paths.length }) });
		},
		onError: (error, paths) => reportError(error, { retry: () => retry.run?.(paths) })
	}));
	retry.run = (paths) => mutation.mutate(paths);
	return mutation;
}

/** 無視が外れている記録を、もう一度無視する（提案を承諾した） */
export function useReapplySkipWorktree(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (paths: string[]) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (paths: string[]) => api.reapplySkipWorktree(getProjectId(), paths),
		onSuccess: async () => {
			await invalidateIgnore(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.skip_reapplied') });
			onDone?.();
		},
		onError: (error, paths) => reportError(error, { retry: () => retry.run?.(paths) })
	}));
	retry.run = (paths) => mutation.mutate(paths);
	return mutation;
}

/**
 * 退避を別名で取り出す（元のファイルは変えない。取り出せたら退避は消え、一覧から外れる）。
 * 成功のトーストには「場所を開く」を付ける。失敗は 3 要素のエラーで出す。
 */
export function useExtractSkipStash(getProjectId: () => string) {
	const client = useQueryClient();
	const retry: { run?: (stashId: string) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (stashId: string) => api.extractSkipStash(getProjectId(), stashId),
		onSuccess: async (result) => {
			const projectId = getProjectId();
			await Promise.all([
				invalidateProject(client, projectId),
				client.invalidateQueries({ queryKey: keys.skipStash(projectId) }),
				client.invalidateQueries({ queryKey: keys.skipWorktree(projectId) })
			]);
			pushToast({
				type: 'success',
				message: t('skip_stash.extracted', { name: baseName(result.copyPath) }),
				actionLabel: t('toast.open_location'),
				onaction: () => {
					void api
						.openFile(projectId, result.copyPath, 'folder')
						.catch((error: unknown) => reportError(error));
				}
			});
		},
		onError: (error, stashId) => reportError(error, { retry: () => retry.run?.(stashId) })
	}));
	retry.run = (stashId) => mutation.mutate(stashId);
	return mutation;
}

/** 再適用の提案で「今はしない」を選ぶ（記録に印を立て、同じ提案を繰り返さない） */
export function useDismissSkipDrift(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const mutation = createMutation(() => ({
		mutationFn: (paths: string[]) => api.dismissSkipWorktreeDrift(getProjectId(), paths),
		onSuccess: async () => {
			await client.invalidateQueries({ queryKey: keys.skipDrift(getProjectId()) });
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
	return mutation;
}

/** 拡張機能の一覧を取り直す（有効・無効、追加、除外の後。反映は次回の起動から） */
function invalidateExtensions(client: QueryClient): Promise<unknown> {
	return client.invalidateQueries({ queryKey: keys.extensions });
}

/** 追加分の有効・無効を変える（設計書 11.9）。反映は次回の起動から */
export function useSetExtensionEnabled() {
	const client = useQueryClient();
	const retry: { run?: (input: { id: string; enabled: boolean }) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
			api.setExtensionEnabled(id, enabled),
		onSuccess: async () => {
			await invalidateExtensions(client);
			pushToast({ type: 'success', message: t('toast.extension_changed') });
		},
		onError: (error, input) => reportError(error, { retry: () => retry.run?.(input) })
	}));
	retry.run = (input) => mutation.mutate(input);
	return mutation;
}

/** フォルダから拡張を追加する（開発者モードのときだけ。設計書 11.9）。元のフォルダは変更しない */
export function useInstallExtension() {
	const client = useQueryClient();
	const retry: { run?: (path: string) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (path: string) => api.installExtensionFromFolder(path),
		onSuccess: async (installed) => {
			await invalidateExtensions(client);
			pushToast({ type: 'success', message: t('toast.extension_added', { name: installed.name }) });
		},
		onError: (error, path) => reportError(error, { retry: () => retry.run?.(path) })
	}));
	retry.run = (path) => mutation.mutate(path);
	return mutation;
}

/** zip から拡張を追加する（開発者モードのときだけ。設計書 11.9）。元の zip は変更しない */
export function useInstallExtensionFromZip() {
	const client = useQueryClient();
	const retry: { run?: (path: string) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (path: string) => api.installExtensionFromZip(path),
		onSuccess: async (installed) => {
			await invalidateExtensions(client);
			pushToast({ type: 'success', message: t('toast.extension_added', { name: installed.name }) });
		},
		onError: (error, path) => reportError(error, { retry: () => retry.run?.(path) })
	}));
	retry.run = (path) => mutation.mutate(path);
	return mutation;
}

/** 追加分を一覧から外す（フォルダは削除せず退避へ移す。設計書 11.9） */
export function useRemoveExtension() {
	const client = useQueryClient();
	const retry: { run?: (input: { id: string; name: string }) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: ({ id }: { id: string; name: string }) => api.removeExtension(id),
		onSuccess: async (_result, { name }) => {
			await invalidateExtensions(client);
			pushToast({ type: 'success', message: t('toast.extension_removed', { name }) });
		},
		onError: (error, input) => reportError(error, { retry: () => retry.run?.(input) })
	}));
	retry.run = (input) => mutation.mutate(input);
	return mutation;
}
