import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useMemoSuggestion(getProjectId: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.memoSuggestion(getProjectId()),
		queryFn: () => api.suggestMemo(getProjectId()),
		enabled: getEnabled() && getProjectId() !== ''
	}));
}

export function useChanges(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.changes(getProjectId()),
		queryFn: () => api.listChanges(getProjectId()),
		enabled: getProjectId() !== ''
	}));
}
