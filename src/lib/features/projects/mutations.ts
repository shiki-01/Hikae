import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { AddProjectInput, Project } from '#lib/api/types.js';
import { reportError } from '#lib/features/notifications/store.svelte.js';

export function useAddProject(onAdded?: (project: Project) => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (input: AddProjectInput) => api.addProject(input),
		onSuccess: async (project) => {
			await client.invalidateQueries({ queryKey: keys.projects });
			onAdded?.(project);
		},
		onError: (error) => reportError(error)
	}));
}

export function useRemoveProject(onDone?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (id: string) => api.removeProject(id),
		onSuccess: async () => {
			await client.invalidateQueries({ queryKey: keys.projects });
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
}

export function useRelocateProject(onDone?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: async (id: string) => api.relocateProject(id, await api.pickFolder()),
		onSuccess: async () => {
			await client.invalidateQueries({ queryKey: keys.projects });
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
}

export function useCompleteOnboarding() {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: () => api.completeOnboarding(),
		onSuccess: () => client.invalidateQueries({ queryKey: keys.session })
	}));
}
