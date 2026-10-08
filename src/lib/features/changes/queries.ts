import { createQuery } from '@tanstack/svelte-query';
import { api, isTauri } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

/** 変更一覧を自動で取り直す間隔（ms）。外部アプリでのファイル編集に追従するため */
export const CHANGES_REFETCH_INTERVAL_MS = 5000;

export function useMemoSuggestion(getProjectId: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.memoSuggestion(getProjectId()),
		queryFn: () => api.suggestMemo(getProjectId()),
		enabled: getEnabled() && getProjectId() !== ''
	}));
}

/**
 * 変更一覧。画面を開いている間は一定間隔で取り直し、ウィンドウに戻ったときにも取り直す
 * （ファイル監視を使うまでの代わり）。裏に隠れている間は止まる。モックでは取り直さない。
 */
export function useChanges(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.changes(getProjectId()),
		queryFn: () => api.listChanges(getProjectId()),
		enabled: getProjectId() !== '',
		refetchInterval: isTauri ? CHANGES_REFETCH_INTERVAL_MS : false,
		refetchOnWindowFocus: isTauri
	}));
}
