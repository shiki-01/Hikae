import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

/** アプリ全体の健全性（保存に必要な部品が使えるか）。起動中は変わらないため、取り直さない */
export function useAppHealth() {
	return createQuery(() => ({
		queryKey: keys.appHealth,
		queryFn: () => api.getAppHealth(),
		staleTime: Infinity
	}));
}
