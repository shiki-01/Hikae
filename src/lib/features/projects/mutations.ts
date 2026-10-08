import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type {
	AddProjectInput,
	AddProjectResult,
	ClonePhase,
	ConnectRemoteInput,
	Project,
	RemoteOutcome,
	SizeChoice
} from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import {
	connectInputFromAdd,
	followUpFor,
	type FollowUpContext,
	type RemoteFollowUp
} from './remote-follow-up';
import { startRemoteFollowUp } from './remote-follow-up.svelte';
import { invalidateProject } from './sync';

/**
 * 保存先（GitHub）を作って接続した結果を利用者に伝える。
 * 途中で失敗した場合も、ローカルのファイルは無事なので、3 要素のエラーで次の行動を示す。
 */
export function announceRemote(outcome: RemoteOutcome | null, context?: FollowUpContext): void {
	if (outcome === null) return;
	// 保存先を作る前に利用者の判断が必要なとき（同名の空の保存先、大きいファイル）は、確認の続きに移る
	const followUp = context ? followUpFor(outcome, context) : null;
	if (followUp) {
		startRemoteFollowUp(followUp);
		return;
	}
	if (outcome.error) {
		reportError(outcome.error);
		return;
	}
	if (outcome.sizeCheck && outcome.connected) {
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
		onSuccess: async (result, input) => {
			await client.invalidateQueries({ queryKey: keys.projects });
			announceRemote(result.remote, {
				projectId: result.project.id,
				projectName: result.project.name,
				input: connectInputFromAdd(input)
			});
			onAdded?.(result.project);
		},
		onError: (error) => reportError(error)
	}));
}

/** ローカルだけのプロジェクトを GitHub に接続する。作成前の失敗は例外（エラーダイアログ）、作成後の失敗は結果で返る */
export function useConnectRemote(onDone?: (outcome: RemoteOutcome) => void, onFail?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (args: { id: string; name: string; input: ConnectRemoteInput }) =>
			api.connectRemote(args.id, args.input),
		onSuccess: async (outcome, args) => {
			await invalidateProject(client, args.id);
			announceRemote(outcome, { projectId: args.id, projectName: args.name, input: args.input });
			onDone?.(outcome);
		},
		onError: (error) => {
			reportError(error);
			onFail?.();
		}
	}));
}

/**
 * 作成の前に見つかった大きいファイルについての選択を保存に反映してから、接続をやり直す。
 * 保存してもまだ確認が必要なら、確認の続きに戻る（接続はしない）。
 */
export function useConnectAfterSizeChoice(onDone?: () => void, onFail?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: async (args: {
			followUp: Extract<RemoteFollowUp, { kind: 'size' }>;
			choice: SizeChoice;
		}) => {
			const { projectId, input } = args.followUp;
			const saved = await api.saveWithSizeChoice(
				projectId,
				t('connect.first_save_memo'),
				args.choice
			);
			if (saved.kind === 'size_check') return { check: saved.check, outcome: null };
			return { check: null, outcome: await api.connectRemote(projectId, input) };
		},
		onSuccess: async (result, { followUp }) => {
			await invalidateProject(client, followUp.projectId);
			if (result.check) startRemoteFollowUp({ ...followUp, check: result.check });
			else {
				announceRemote(result.outcome, {
					projectId: followUp.projectId,
					projectName: followUp.projectName,
					input: followUp.input
				});
			}
			onDone?.();
		},
		onError: (error) => {
			reportError(error);
			onFail?.();
		}
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
