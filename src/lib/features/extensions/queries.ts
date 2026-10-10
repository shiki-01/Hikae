import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { MediaFetch } from './compare-payload.js';

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

/**
 * 比較に使う画像の中身を app-blob から読む。404（その時点にファイルが無い）と、その他の失敗（読めない）を
 * 区別して返す（エラーにはしない）。読み終えたら保持しない
 */
export function useSideMedia(getUrl: () => string, getEnabled: () => boolean = () => true) {
	return createQuery(() => ({
		queryKey: keys.compareMedia(getUrl()),
		queryFn: async (): Promise<MediaFetch> => {
			try {
				const response = await fetch(getUrl(), { cache: 'no-store' });
				if (response.status === 404) {
					await response.body?.cancel();
					return { status: 'absent' };
				}
				if (!response.ok) {
					await response.body?.cancel();
					return { status: 'error' };
				}
				return { status: 'ok', data: await response.arrayBuffer() };
			} catch {
				return { status: 'error' };
			}
		},
		enabled: getEnabled(),
		gcTime: 0,
		retry: false
	}));
}
