import { onDestroy } from 'svelte';
import type { QueryClient } from '@tanstack/svelte-query';
import { isTauri } from '#lib/api/index.js';
import { events } from '#lib/bindings.js';
import { applyLiveEvent, initialLiveState, type LiveEvent, type LiveState } from './live-state';
import { invalidateProject } from './sync';

const states = $state<Record<string, LiveState>>({});

/** プロジェクトの実行時の状態（イベント未受信なら初期値） */
export function liveStateOf(projectId: string): LiveState {
	return states[projectId] ?? initialLiveState;
}

/**
 * バックエンドのイベントを購読し、状態の反映と Query の無効化を行う。
 * Tauri 実行時のみ有効で、ブラウザ（モック）では何もしない。コンポーネント破棄時に解除する。
 */
export function useBackendEvents(client: QueryClient): void {
	if (!isTauri) return;
	let disposed = false;
	const unlisteners: (() => void)[] = [];

	function dispatch(projectId: string, event: LiveEvent) {
		const update = applyLiveEvent(states[projectId] ?? initialLiveState, event);
		states[projectId] = update.state;
		if (update.invalidate) void invalidateProject(client, projectId);
	}

	function subscribe(register: () => Promise<() => void>) {
		void register().then((unlisten) => {
			// 登録が終わる前に破棄された場合はすぐ解除する
			if (disposed) unlisten();
			else unlisteners.push(unlisten);
		});
	}

	subscribe(() =>
		events.statusChanged.listen((e) =>
			dispatch(e.payload.project_id, { type: 'status-changed', payload: e.payload })
		)
	);
	subscribe(() =>
		events.opProgress.listen((e) =>
			dispatch(e.payload.project_id, { type: 'op-progress', payload: e.payload })
		)
	);
	subscribe(() =>
		events.opFinished.listen((e) =>
			dispatch(e.payload.project_id, { type: 'op-finished', payload: e.payload })
		)
	);
	subscribe(() =>
		events.syncStateChanged.listen((e) =>
			dispatch(e.payload.project_id, { type: 'sync-state-changed', payload: e.payload })
		)
	);
	subscribe(() =>
		events.needsAttention.listen((e) =>
			dispatch(e.payload.project_id, { type: 'needs-attention', payload: e.payload })
		)
	);

	onDestroy(() => {
		disposed = true;
		for (const unlisten of unlisteners) unlisten();
		unlisteners.length = 0;
	});
}
