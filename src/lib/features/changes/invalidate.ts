import type { QueryClient } from '@tanstack/svelte-query';
import { keys } from '#lib/api/keys.js';

/**
 * ファイル監視の通知（`files-changed`）を受けたときの無効化。
 * 変更一覧と、そこから作る保存メモの案だけを取り直す。履歴・プロジェクト一覧など
 * git を多く呼ぶ取得は、保存などの操作の完了通知（`status-changed`）のときだけ行う。
 */
export function invalidateChanges(client: QueryClient, projectId: string): Promise<unknown> {
	return Promise.all([
		client.invalidateQueries({ queryKey: keys.changes(projectId) }),
		client.invalidateQueries({ queryKey: keys.memoSuggestion(projectId) })
	]);
}
