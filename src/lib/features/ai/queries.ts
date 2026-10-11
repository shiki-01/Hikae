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
