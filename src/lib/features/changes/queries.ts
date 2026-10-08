import { createQuery } from '@tanstack/svelte-query';
import { api, isTauri } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import { changesRefetchInterval } from './refetch';

export function useMemoSuggestion(getProjectId: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.memoSuggestion(getProjectId()),
		queryFn: () => api.suggestMemo(getProjectId()),
		enabled: getEnabled() && getProjectId() !== ''
	}));
}

/**
 * 変更一覧。ファイル監視が動いている間は、バックエンドの通知（`files-changed`）で取り直すため
 * 定期的には取り直さない。監視が動いていない（失敗・未開始・非対応）ときだけ、フォールバックとして
 * 30 秒間隔で取り直す。ウィンドウに戻ったときは常に取り直す。裏に隠れている間は止まる。
 * モックでは取り直さない。
 */
export function useChanges(getProjectId: () => string, getWatching: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.changes(getProjectId()),
		queryFn: () => api.listChanges(getProjectId()),
		enabled: getProjectId() !== '',
		refetchInterval: changesRefetchInterval(isTauri, getWatching()),
		refetchOnWindowFocus: isTauri
	}));
}

/**
 * 「すべてのファイル」の一覧。表示している間だけ取得し、ファイル監視の通知（`files-changed`）で取り直す
 * （無効化は `invalidateChanges`）。ブラウザ単体（モック）では取り直さない。
 */
export function useProjectTree(getProjectId: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.projectTree(getProjectId()),
		queryFn: () => api.listProjectTree(getProjectId()),
		enabled: getEnabled() && getProjectId() !== '',
		refetchOnWindowFocus: isTauri
	}));
}
