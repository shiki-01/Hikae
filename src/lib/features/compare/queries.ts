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

/** docx・xlsx の内容の比較。抽出できないものは `unavailable` の結果になる（エラーにはしない） */
export function useCompareOffice(
	getProjectId: () => string,
	getPath: () => string | null,
	getFrom: () => string,
	getTo: () => string,
	getEnabled: () => boolean = () => true
) {
	return createQuery(() => ({
		queryKey: keys.compareOffice(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		queryFn: () => api.compareOffice(getProjectId(), getPath() ?? '', getFrom(), getTo()),
		enabled: getProjectId() !== '' && getPath() !== null && getEnabled()
	}));
}
