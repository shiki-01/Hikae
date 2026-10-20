import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';

export function useProjects() {
	return createQuery(() => ({ queryKey: keys.projects, queryFn: () => api.listProjects() }));
}

export function useProject(getId: () => string) {
	return createQuery(() => ({
		queryKey: keys.project(getId()),
		queryFn: () => api.getProject(getId()),
		enabled: getId() !== ''
	}));
}

/** 作業コピーの一覧（設計書 4.9）。既定は「しまった」ものを除く。画面はこの機能がオンのときだけ使う */
export function useWorkCopies(getId: () => string) {
	return createQuery(() => ({
		queryKey: keys.workCopies(getId()),
		queryFn: () => api.listWorkCopies(getId(), false),
		enabled: getId() !== ''
	}));
}

export function useOwners() {
	return createQuery(() => ({ queryKey: keys.owners, queryFn: () => api.listOwners() }));
}

export function useRemoteProjects(getQuery: () => string, getEnabled: () => boolean) {
	return createQuery(() => ({
		queryKey: keys.remoteProjects(getQuery()),
		queryFn: () => api.listRemoteProjects(getQuery()),
		enabled: getEnabled()
	}));
}

export function useSession() {
	return createQuery(() => ({
		queryKey: keys.session,
		queryFn: () => api.getSession(),
		staleTime: Infinity
	}));
}
