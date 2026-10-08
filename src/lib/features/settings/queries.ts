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
