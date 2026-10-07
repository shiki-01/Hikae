import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { AppSettings } from '#lib/api/types.js';
import { reportError } from '#lib/features/notifications/store.svelte.js';

export function useSettings() {
	return createQuery(() => ({ queryKey: keys.settings, queryFn: () => api.getSettings() }));
}

export function useUpdateSettings() {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (patch: Partial<AppSettings>) => api.updateSettings(patch),
		onMutate: async (patch) => {
			await client.cancelQueries({ queryKey: keys.settings });
			const previous = client.getQueryData<AppSettings>(keys.settings);
			if (previous) client.setQueryData(keys.settings, { ...previous, ...patch });
			return { previous };
		},
		onError: (error, _patch, context) => {
			if (context?.previous) client.setQueryData(keys.settings, context.previous);
			reportError(error);
		},
		onSettled: () => client.invalidateQueries({ queryKey: keys.settings })
	}));
}
