import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { AddFilesOutcome, DroppedFile, SizeCheck, SizeChoice } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';

export interface SaveInput {
	memo: string;
	/** 大きいファイルについての選択。最初の保存では付けない */
	choice?: SizeChoice;
}

/**
 * 保存。大きいファイルがあると保存されずに確認が返るため、`onSizeCheck` で選択を求める。
 * その場合は保存した扱いにしない（一覧の再取得も完了の通知も出さない）。
 */
export function useSave(
	getProjectId: () => string,
	onSaved?: () => void,
	onSizeCheck?: (memo: string, check: SizeCheck) => void
) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({ memo, choice }: SaveInput) =>
			choice
				? api.saveWithSizeChoice(getProjectId(), memo, choice)
				: api.save(getProjectId(), memo),
		onSuccess: async (outcome, { memo }) => {
			if (outcome.kind === 'size_check') {
				onSizeCheck?.(memo, outcome.check);
				return;
			}
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'success', message: t('toast.saved') });
			onSaved?.();
		},
		onError: (error) => reportError(error)
	}));
}

/** 結果（別名にしたもの・追加できなかったもの・大きいもの）は `onResult` で受け取り、画面で知らせる */
export function useAddFiles(
	getProjectId: () => string,
	onResult?: (outcome: AddFilesOutcome) => void
) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (files: DroppedFile[]) => api.addFiles(getProjectId(), files),
		onSuccess: async (outcome) => {
			await invalidateProject(client, getProjectId());
			if (outcome.added.length > 0) {
				pushToast({
					type: 'success',
					message: t('toast.files_added', { count: outcome.added.length })
				});
			}
			onResult?.(outcome);
		},
		onError: (error) => reportError(error)
	}));
}
