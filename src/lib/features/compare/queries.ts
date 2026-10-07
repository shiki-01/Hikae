import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useCompare(
	getProjectId: () => string,
	getPath: () => string | null,
	getFrom: () => string,
	getTo: () => string
) {
	return createQuery(() => ({
		queryKey: keys.compare(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		queryFn: () => api.compare(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		enabled: getProjectId() !== '' && getPath() !== null
	}));
}
