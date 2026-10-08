import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type {
	AddProjectInput,
	AddProjectResult,
	ClonePhase,
	ConnectRemoteInput,
	Project,
	RemoteOutcome
} from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { invalidateProject } from './sync';

/**
 * 保存先（GitHub）を作って接続した結果を利用者に伝える。
 * 途中で失敗した場合も、ローカルのファイルは無事なので、3 要素のエラーで次の行動を示す。
 */
export function announceRemote(outcome: RemoteOutcome | null): void {
	if (outcome === null) return;
	if (outcome.error) {
		reportError(outcome.error);
		return;
	}
	if (outcome.sizeCheck) {
		pushToast({ type: 'warning', message: t('toast.first_save_skipped') });
	} else if (outcome.uploaded) {
		pushToast({ type: 'success', message: t('toast.pushed') });
	} else if (outcome.connected) {
		pushToast({ type: 'success', message: t('toast.connected') });
	}
}

export function useAddProject(
	onAdded?: (project: Project) => void,
	onProgress?: (phase: ClonePhase) => void
) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (input: AddProjectInput): Promise<AddProjectResult> =>
			api.addProject(input, onProgress),
		onSuccess: async (result) => {
			await client.invalidateQueries({ queryKey: keys.projects });
			announceRemote(result.remote);
			onAdded?.(result.project);
		},
		onError: (error) => reportError(error)
	}));
}

/** ローカルだけのプロジェクトを GitHub に接続する。作成前の失敗は例外（エラーダイアログ）、作成後の失敗は結果で返る */
export function useConnectRemote(onDone?: (outcome: RemoteOutcome) => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (args: { id: string; input: ConnectRemoteInput }) =>
			api.connectRemote(args.id, args.input),
		onSuccess: async (outcome, args) => {
			await invalidateProject(client, args.id);
			announceRemote(outcome);
			onDone?.(outcome);
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
