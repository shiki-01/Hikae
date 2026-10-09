import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { ConflictEdit, ConflictResolution } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

/** 解消の指示。choices（全件）と、行ごとに編集した全文（ある場合） */
export interface ResolvePayload {
	resolutions: ConflictResolution[];
	edits: ConflictEdit[];
}

export function useResolve(getProjectId: () => string, onResolved?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (payload: ResolvePayload) => void } = {};
	const mutation = createMutation(() => ({
		mutationFn: (payload: ResolvePayload) =>
			api.resolveConflicts(getProjectId(), payload.resolutions, payload.edits),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.resolved') });
			onResolved?.();
		},
		onError: (error, payload) => reportError(error, { retry: () => retry.run?.(payload) })
	}));
	retry.run = (payload) => mutation.mutate(payload);
	return mutation;
}

export function useAbortMerge(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const mutation = createMutation(() => ({
		mutationFn: () => api.abortMerge(getProjectId()),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'info', message: t('toast.merge_aborted') });
			onDone?.();
		},
		onError: (error) => reportError(error, { retry: () => retry.run() })
	}));
	retry.run = () => mutation.mutate();
	return mutation;
}
