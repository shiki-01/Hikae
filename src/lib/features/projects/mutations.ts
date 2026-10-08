import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { AddProjectInput, ClonePhase, Project } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from './sync';

export function useAddProject(
	onAdded?: (project: Project) => void,
	onProgress?: (phase: ClonePhase) => void
) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (input: AddProjectInput) => api.addProject(input, onProgress),
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
		mutationFn: async (id: string) => {
			const folder = await api.pickFolder();
			if (folder === null) return false;
			await api.relocateProject(id, folder);
			return true;
		},
		onSuccess: async (changed: boolean) => {
			if (!changed) return;
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

/** 中断された操作の前の状態へ戻す。成功で関連するクエリを更新し、失敗は 3 要素のエラーで表示する */
export function useRecoverInterrupted(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: () => api.recoverInterrupted(getProjectId()),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.recovered') });
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
}
