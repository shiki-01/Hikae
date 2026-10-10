import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

/** 表示拡張の一覧。起動中は変わらないため、取り直さない */
export function useRenderers() {
	return createQuery(() => ({
		queryKey: keys.renderers,
		queryFn: () => api.listRenderers(),
		staleTime: Number.POSITIVE_INFINITY
	}));
}

/**
 * app-blob の URL から中身（バイト列）を読む。画像・pdf を表示拡張へ渡すために使う。
 * 読み終えたら保持しない（大きいファイルを長く抱えないため）
 */
export function useMediaBytes(getUrl: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.mediaBytes(getUrl()),
		queryFn: async (): Promise<ArrayBuffer> => {
			const response = await fetch(getUrl());
			if (!response.ok) throw new Error(`media ${response.status}`);
			return await response.arrayBuffer();
		},
		enabled: getEnabled(),
		gcTime: 0
	}));
}
