import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useCompare(
	getProjectId: () => string,
	getPath: () => string | null,
	getFrom: () => string,
	getTo: () => string,
	/** 画像のように本文の差分を使わないときは false にして、取得しない */
	getEnabled: () => boolean = () => true
) {
	return createQuery(() => ({
		queryKey: keys.compare(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		queryFn: () => api.compare(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		enabled: getProjectId() !== '' && getPath() !== null && getEnabled()
	}));
}
