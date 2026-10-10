import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

/** AI のモデル一覧と、この PC での導入状態。表示のたびに取り直す */
export function useAiCatalog() {
	return createQuery(() => ({
		queryKey: keys.aiCatalog,
		queryFn: () => api.aiCatalog()
	}));
}

/**
 * 保存メモの AI 提案。`getKey` は変更一覧と設定の組み合わせを表す文字列で、変わるたびに取り直す。
 * 提案が得られなかった（null）場合も結果として扱い、エラー表示はしない。
 */
export function useAiSuggestion(
	getProjectId: () => string,
	getKey: () => string,
	getEnabled: () => boolean
) {
	return createQuery(() => ({
		queryKey: keys.aiSuggestion(getProjectId(), getKey()),
		queryFn: () => api.aiSuggestMemo(getProjectId()),
		enabled: getEnabled() && getProjectId() !== '',
		retry: false
	}));
}
