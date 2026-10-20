import { goto } from '$app/navigation';
import { resolve } from '$app/paths';
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
	RepairOutcome,
	SizeChoice
} from '#lib/api/types.js';
import { AppError } from '#lib/api/errors.js';
import { t, type MessageKey } from '#lib/i18n/index.js';
import { primaryKindOfBackendCode } from '#lib/features/notifications/error-view.js';
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
		// 保存先の設定に関わるエラーは、プロジェクトの画面の接続のダイアログへ誘導できる
		const projectId = context?.projectId;
		const code = outcome.error.backend?.code;
		const guide =
			projectId !== undefined && code !== undefined && primaryKindOfBackendCode(code) === 'guide';
		reportError(
			outcome.error,
			guide
				? {
						onprimary: () =>
							void goto(`${resolve('/project')}?id=${encodeURIComponent(projectId)}&connect=1`)
					}
				: {}
		);
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

/**
 * 途中で止まった操作（中断）への対応の 2 つ。「操作前の状態に戻す」と「このまま続ける」。
 * 成功で関連するクエリを更新する。失敗は 3 要素のエラーで表示し、復元点が無いときは
 * 「このまま続ける」を主ボタンとして出す（設計書 6.3）
 */
export function useInterruptedActions(getProjectId: () => string, onDone?: () => void) {
	const client = useQueryClient();
	const retry = { run: () => {} };
	const dismiss = createMutation(() => ({
		mutationFn: () => api.dismissInterrupted(getProjectId()),
		onSuccess: async () => {
			await invalidateProject(client, getProjectId());
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
	const recover = createMutation(() => ({
		mutationFn: () => api.recoverInterrupted(getProjectId()),
		onSuccess: async (result) => {
			await invalidateProject(client, getProjectId());
			pushToast(
				result === 'nothing-changed'
					? { type: 'info', message: t('interrupted.nothing_changed') }
					: { type: 'success', message: t('toast.recovered') }
			);
			onDone?.();
		},
		onError: (error) =>
			reportError(error, {
				retry: () => retry.run(),
				onprimary: () => dismiss.mutate()
			})
	}));
	retry.run = () => recover.mutate();
	return { recover, dismiss };
}

/** 戻せなかったファイルの名前を、知らせる件数までに絞る */
const REPAIR_LIST_MAX = 5;

/**
 * 「取得し直す」の結果を利用者に伝える。
 * 前提を満たさなかったときは理由を案内する（未ログインはログインし直す案内）。
 * 復旧できたときは、退避フォルダの場所と、コピーし直せなかったファイルを知らせる（退避フォルダに残っている）。
 */
function announceRepair(outcome: RepairOutcome): void {
	if (outcome.reason !== null) {
		if (outcome.reason === 'not_logged_in') {
			reportError(new AppError('E01', 'repair: not logged in'));
			return;
		}
		pushToast({
			type: 'info',
			message: t(`repair.reason.${outcome.reason}` as MessageKey)
		});
		return;
	}
	const path = outcome.asidePath ?? '';
	pushToast({ type: 'success', message: t('repair.done', { path }) });
	if (outcome.failed.length > 0) {
		const shown = outcome.failed.slice(0, REPAIR_LIST_MAX).join('、');
		const rest = outcome.failed.length - REPAIR_LIST_MAX;
		const files = rest > 0 ? `${shown} ${t('repair.more', { count: rest })}` : shown;
		pushToast({
			type: 'warning',
			message: t('repair.copy_failed', { count: outcome.failed.length, path, files })
		});
	}
}

/**
 * 破損リポジトリの「取得し直す」（設計書 5.2 E10）。直列キューの中で行われ、
 * 前提（再検査・保存先・ログイン）を満たさなければ何も変えずに理由を案内する。
 * 失敗は 3 要素のエラーで表示する（作業フォルダは退避フォルダへ移すだけで消さない）
 */
export function useRepairBroken() {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: (projectId: string) => api.repairBrokenProject(projectId),
		onSuccess: async (outcome, projectId) => {
			await invalidateProject(client, projectId);
			announceRepair(outcome);
		},
		onError: (error) => reportError(error)
	}));
}
