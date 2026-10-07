import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useChanges(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.changes(getProjectId()),
		queryFn: () => api.listChanges(getProjectId()),
		enabled: getProjectId() !== ''
	}));
}
