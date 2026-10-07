import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { DroppedFile } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

export function useSave(getProjectId: () => string, onSaved?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (memo: string) => api.save(getProjectId(), memo),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.saved') });
			onSaved?.();
		},
		onError: (error) => reportError(error)
	}));
}

export function useAddFiles(getProjectId: () => string) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (files: DroppedFile[]) => api.addFiles(getProjectId(), files),
		onSuccess: async (count) => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.files_added', { count }) });
		},
		onError: (error) => reportError(error)
	}));
}
