import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

/** ダウンロード中に一覧を取り直す間隔（ミリ秒） */
const DOWNLOADING_REFETCH_MS = 2000;

/**
 * AI のモデル一覧と、この PC での導入状態。表示のたびに取り直す。
 * ダウンロード中のモデルがある間は、定期的にも取り直す（進捗・終了の通知を受け損ねても、表示が止まらないように）
 */
export function useAiCatalog() {
	return createQuery(() => ({
		queryKey: keys.aiCatalog,
		queryFn: () => api.aiCatalog(),
		refetchInterval: (query) =>
			query.state.data?.models.some((m) => m.downloading) ? DOWNLOADING_REFETCH_MS : false
	}));
}
