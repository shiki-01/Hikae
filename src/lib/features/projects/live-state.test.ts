import { describe, it, expect } from 'vitest';
import {
	applyLiveEvent,
	hasInterruptedOperation,
	initialLiveState,
	syncingKind,
	type LiveState
} from './live-state';

const base: LiveState = initialLiveState;

describe('イベントの反映', () => {
	it('状態変化では状態を変えず、Query を無効化する', () => {
		const result = applyLiveEvent(base, { type: 'status-changed', payload: { project_id: 'p' } });
		expect(result).toEqual({ state: base, invalidate: true });
	});

	it('操作の開始で実行中になり、終了で解除される', () => {
		const started = applyLiveEvent(base, {
			type: 'op-progress',
			payload: { project_id: 'p', operation: 'pull', trigger: 'auto' }
		});
		expect(started.state.operation).toBe('pull');
		expect(started.invalidate).toBe(false);

		const finished = applyLiveEvent(started.state, {
			type: 'op-finished',
			payload: { project_id: 'p', operation: 'pull', trigger: 'auto', ok: true, outcome: 'merged' }
		});
		expect(finished.state.operation).toBeNull();
		expect(finished.invalidate).toBe(true);
	});

	it('失敗した操作の終了では要対応を残し、成功では消す', () => {
		const attention: LiveState = { ...base, operation: 'push', attention: 'auth' };
		const failed = applyLiveEvent(attention, {
			type: 'op-finished',
			payload: { project_id: 'p', operation: 'push', trigger: 'manual', ok: false, outcome: 'x' }
		});
		expect(failed.state.attention).toBe('auth');
		const ok = applyLiveEvent(attention, {
			type: 'op-finished',
			payload: { project_id: 'p', operation: 'push', trigger: 'manual', ok: true, outcome: 'x' }
		});
		expect(ok.state.attention).toBeNull();
	});

	it('同期状態の変化を保持し、待機に戻ると要対応を消す', () => {
		const offline = applyLiveEvent(base, {
			type: 'sync-state-changed',
			payload: { project_id: 'p', state: 'offline', retry_at: 1 }
		});
		expect(offline.state.sync).toBe('offline');
		expect(offline.invalidate).toBe(true);

		const withAttention: LiveState = { ...offline.state, attention: 'unsaved-changes' };
		const idle = applyLiveEvent(withAttention, {
			type: 'sync-state-changed',
			payload: { project_id: 'p', state: 'idle', retry_at: null }
		});
		expect(idle.state).toEqual({ operation: null, sync: 'idle', attention: null });
	});

	it('要対応の通知で理由を保持する', () => {
		const result = applyLiveEvent(base, {
			type: 'needs-attention',
			payload: { project_id: 'p', reason: 'conflict' }
		});
		expect(result.state.attention).toBe('conflict');
		expect(result.invalidate).toBe(true);
	});

	it('途中で止まった操作は、同期が待機に戻っても残り、操作の成功で消える', () => {
		const stopped = applyLiveEvent(base, {
			type: 'needs-attention',
			payload: { project_id: 'p', reason: 'interrupted-operation' }
		});
		expect(stopped.state.attention).toBe('interrupted-operation');
		const idle = applyLiveEvent(stopped.state, {
			type: 'sync-state-changed',
			payload: { project_id: 'p', state: 'idle', retry_at: null }
		});
		expect(idle.state.attention).toBe('interrupted-operation');
		const failed = applyLiveEvent(idle.state, {
			type: 'op-finished',
			payload: {
				project_id: 'p',
				operation: 'pull',
				trigger: 'manual',
				ok: false,
				outcome: 'error'
			}
		});
		expect(failed.state.attention).toBe('interrupted-operation');
		const ok = applyLiveEvent(failed.state, {
			type: 'op-finished',
			payload: {
				project_id: 'p',
				operation: 'pull',
				trigger: 'manual',
				ok: true,
				outcome: 'merged'
			}
		});
		expect(ok.state.attention).toBeNull();
	});

	it('元の状態を書き換えない', () => {
		const before = { ...base };
		applyLiveEvent(base, {
			type: 'needs-attention',
			payload: { project_id: 'p', reason: 'auth' }
		});
		expect(base).toEqual(before);
	});
});

describe('処理中の種別', () => {
	it('操作名を表示用の種別に対応づける', () => {
		expect(syncingKind(null)).toBeNull();
		expect(syncingKind('pull')).toBe('fetch');
		expect(syncingKind('push')).toBe('push');
		expect(syncingKind('save')).toBe('other');
	});
});

describe('途中で止まった操作の判定', () => {
	it('通知かプロジェクトの状態のどちらかが示せば残っているとみなす', () => {
		expect(hasInterruptedOperation(base, null)).toBe(false);
		expect(hasInterruptedOperation(base, 'save')).toBe(true);
		expect(hasInterruptedOperation({ ...base, attention: 'interrupted-operation' }, null)).toBe(
			true
		);
		expect(hasInterruptedOperation({ ...base, attention: 'auth' }, null)).toBe(false);
	});
});
