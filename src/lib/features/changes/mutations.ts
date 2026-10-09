import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type { AddFilesOutcome, DroppedFile, SizeCheck, SizeChoice } from '#lib/api/types.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from '#lib/features/projects/sync.js';
import { fileNameOf } from './discard';

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
	const retry: { run?: (input: SaveInput) => void } = {};
	const mutation = createMutation(() => ({
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
		onError: (error, input) => reportError(error, { retry: () => retry.run?.(input) })
	}));
	retry.run = (input) => mutation.mutate(input);
	return mutation;
}

/** 結果（別名にしたもの・追加できなかったもの・大きいもの）は `onResult` で受け取り、画面で知らせる */
export function useAddFiles(
	getProjectId: () => string,
	onResult?: (outcome: AddFilesOutcome) => void
) {
	const client = useQueryClient();
	const retry: { run?: (files: DroppedFile[]) => void } = {};
	const mutation = createMutation(() => ({
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
		onError: (error, files) => reportError(error, { retry: () => retry.run?.(files) })
	}));
	retry.run = (files) => mutation.mutate(files);
	return mutation;
}

/**
 * 新規ファイルを「元に戻す（作成しない）」。成功すると、ファイルはごみ箱に移っている。
 * 取り消すと、実行前の復元点（自動保存）の内容に戻る（ごみ箱からではなく、アプリ内の控えから戻す）。
 */
export function useDiscardNewFile(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry: { run?: (path: string) => void } = {};

	const undo = createMutation(() => ({
		mutationFn: (undoToken: string) => api.undoRestore(getProjectId(), undoToken),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'info', message: t('toast.undone') });
		},
		onError: (error) => reportError(error)
	}));

	const mutation = createMutation(() => ({
		mutationFn: (path: string) => api.discardNewFile(getProjectId(), path),
		onSuccess: async (result, path) => {
			await invalidateProject(client, getProjectId());
			const undoToken = result.undoToken;
			pushToast({
				type: 'success',
				message: t('toast.discarded', { name: fileNameOf(path) }),
				...(undoToken
					? { actionLabel: t('toast.undo'), onaction: () => undo.mutate(undoToken) }
					: {})
			});
			onDone?.();
		},
		// 断られたとき（大きいファイル・保存済み・ごみ箱が使えないなど）は、ファイルは削除されていない
		onError: (error, path) => {
			onDone?.();
			reportError(error, { retry: () => retry.run?.(path) });
		}
	}));
	retry.run = (path) => mutation.mutate(path);
	return mutation;
}
