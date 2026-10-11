import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { AppSettings, SettingsView } from '#lib/api/types.js';
import { applyPatch } from './settings-model';
import { reportError } from '#lib/features/notifications/store.svelte.js';

/** `getProjectId` が null なら全体設定、指定があればそのプロジェクトの上書きを反映した設定 */
export function useSettings(getProjectId: () => string | null) {
	return createQuery(() => ({
		queryKey: keys.settings(getProjectId()),
		queryFn: () => api.getSettings(getProjectId())
	}));
}

export function useUpdateSettings(getProjectId: () => string | null) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (patch: Partial<AppSettings>) => api.updateSettings(getProjectId(), patch),
		onMutate: async (patch) => {
			const key = keys.settings(getProjectId());
			await client.cancelQueries({ queryKey: key });
			const previous = client.getQueryData<SettingsView>(key);
			if (previous) client.setQueryData(key, applyPatch(previous, patch, getProjectId() !== null));
			return { previous, key };
		},
		onError: (error, _patch, context) => {
			if (context?.previous) client.setQueryData(context.key, context.previous);
			reportError(error);
		},
		onSettled: () => client.invalidateQueries({ queryKey: keys.settingsAll })
	}));
}

/** 保存しないファイルのパターン一覧（件数つき）。プロジェクトを開いているときだけ使う */
export function useIgnoreRules(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.ignoreRules(getProjectId()),
		queryFn: () => api.listIgnoreRules(getProjectId())
	}));
}

/** 追跡中（保存対象になっている）で、保存しないファイルに当たるファイル */
export function useTrackedIgnoredFiles(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.trackedIgnored(getProjectId()),
		queryFn: () => api.listTrackedIgnoredFiles(getProjectId())
	}));
}

/** 「今後の変更だけ無視する」の記録と、この PC での現在の状態（設計書 4.8、S6） */
export function useSkipWorktreeEntries(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.skipWorktree(getProjectId()),
		queryFn: () => api.listSkipWorktree(getProjectId())
	}));
}

/** 退避に残っているファイル（設計書 4.8。取り出すと一覧から消える） */
export function useSkipStash(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.skipStash(getProjectId()),
		queryFn: () => api.listSkipStash(getProjectId())
	}));
}

/** 無視が外れている記録（再適用の提案の対象。提案を出すかどうかは呼び出し側が決める） */
export function useSkipDrift(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.skipDrift(getProjectId()),
		queryFn: () => api.listSkipWorktreeDrift(getProjectId())
	}));
}
