import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useConflicts(getProjectId: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.conflicts(getProjectId()),
		queryFn: () => api.listConflicts(getProjectId()),
		enabled: getProjectId() !== '' && getEnabled()
	}));
}
