// モバイル版のデータ取得（TanStack Query）。書き込み（アップロード・ダウンロード）は画面の操作から直接呼ぶ。

import { createQuery } from '@tanstack/svelte-query';
import { mobileApi } from '#lib/api/mobile-index.js';

export const mobileKeys = {
	account: ['mobile', 'account'] as const,
	repos: ['mobile', 'repos'] as const,
	dir: (owner: string, name: string, path: string) => ['mobile', 'dir', owner, name, path] as const,
	preview: (owner: string, name: string, path: string, sha: string, ref: string | null) =>
		['mobile', 'preview', owner, name, path, sha, ref] as const,
	history: (owner: string, name: string, path: string) =>
		['mobile', 'history', owner, name, path] as const
};

export function useMobileAccount() {
	return createQuery(() => ({
		queryKey: mobileKeys.account,
		queryFn: () => mobileApi.getAccount(),
		staleTime: Infinity
	}));
}

export function useRepos() {
	return createQuery(() => ({
		queryKey: mobileKeys.repos,
		queryFn: () => mobileApi.listRepos()
	}));
}

export function useDirListing(
	getOwner: () => string,
	getName: () => string,
	getPath: () => string,
	getEnabled: () => boolean = () => true
) {
	return createQuery(() => ({
		queryKey: mobileKeys.dir(getOwner(), getName(), getPath()),
		queryFn: () => mobileApi.listDir(getOwner(), getName(), getPath()),
		enabled: getEnabled()
	}));
}

/** `getRef` は過去の版のコミットの sha。現在の版なら null */
export function usePreview(
	getOwner: () => string,
	getName: () => string,
	getPath: () => string,
	getSha: () => string,
	getRef: () => string | null = () => null
) {
	return createQuery(() => ({
		queryKey: mobileKeys.preview(getOwner(), getName(), getPath(), getSha(), getRef()),
		queryFn: () =>
			mobileApi.preview(getOwner(), getName(), getPath(), getSha(), getRef() ?? undefined),
		// 同じ版の内容は変わらないので、取り直さない
		staleTime: Infinity
	}));
}

export function useFileHistory(
	getOwner: () => string,
	getName: () => string,
	getPath: () => string,
	getEnabled: () => boolean
) {
	return createQuery(() => ({
		queryKey: mobileKeys.history(getOwner(), getName(), getPath()),
		queryFn: () => mobileApi.fileHistory(getOwner(), getName(), getPath()),
		enabled: getEnabled()
	}));
}
