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
