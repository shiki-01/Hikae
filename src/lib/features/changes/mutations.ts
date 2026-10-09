import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import type {
	AddConflictDecision,
	AddFilesOutcome,
	DroppedFile,
	NameConflict,
	SizeCheck,
	SizeChoice
} from '#lib/api/types.js';
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

/** ファイル追加の要求。同名のファイルがあるときは、確認のあとに `decisions` を付けてやり直す */
export interface AddRequest {
	files: DroppedFile[];
	/** 追加先のフォルダ（プロジェクトからの相対パス）。空ならプロジェクト直下 */
	destSubdir: string;
	/** 同名のファイルについての選択。付いているときは、確認せずに実行する */
	decisions?: AddConflictDecision[];
}

/**
 * ファイル・フォルダの追加。同名のファイルがあって何も書かれなかったときは、`onNeedsDecision` で
 * 確認の一覧を受け取り、選択を添えてやり直す。結果（置き換え・別名・追加できなかったもの・リンク・
 * 大きいもの）は `onResult` で受け取り、画面で知らせる。置き換えたときは「取り消す」を付ける。
 */
export function useAddFiles(
	getProjectId: () => string,
	onResult?: (outcome: AddFilesOutcome) => void,
	onNeedsDecision?: (request: AddRequest, conflicts: NameConflict[]) => void,
	onFailed?: () => void
) {
	const client = useQueryClient();
	const retry: { run?: (request: AddRequest) => void } = {};

	const undo = createMutation(() => ({
		mutationFn: (undoToken: string) => api.undoRestore(getProjectId(), undoToken),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			pushToast({ type: 'info', message: t('toast.undone') });
		},
		onError: (error) => reportError(error)
	}));

	const mutation = createMutation(() => ({
		mutationFn: (request: AddRequest) =>
			api.addFiles(getProjectId(), request.files, {
				destSubdir: request.destSubdir,
				// 選択を添えたときは、一覧に無いファイル（確認の後に増えたもの）を上書きしない方針で実行する
				policy: request.decisions ? 'keep_both' : 'ask',
				decisions: request.decisions ?? []
			}),
		onSuccess: async (outcome, request) => {
			// 同名のファイルがあって何も書かれていない。選択を求める
			if (outcome.needsDecision.length > 0) {
				onNeedsDecision?.(request, outcome.needsDecision);
				return;
			}
			await invalidateProject(client, getProjectId());
			const replaced = outcome.added.filter((file) => file.replaced).length;
			if (outcome.added.length > 0) {
				const undoToken = outcome.undoToken;
				pushToast({
					type: 'success',
					message:
						replaced > 0
							? t('toast.files_added_replaced', { count: outcome.added.length, replaced })
							: t('toast.files_added', { count: outcome.added.length }),
					// 置き換えたときだけ、置き換える前の内容に戻せる
					...(undoToken && replaced > 0
						? { actionLabel: t('toast.undo'), onaction: () => undo.mutate(undoToken) }
						: {})
				});
			}
			onResult?.(outcome);
		},
		onError: (error, request) => {
			onFailed?.();
			reportError(error, { retry: () => retry.run?.(request) });
		}
	}));
	retry.run = (request) => mutation.mutate(request);
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
