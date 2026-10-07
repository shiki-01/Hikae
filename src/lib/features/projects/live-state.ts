import type {
	AttentionReason,
	NeedsAttention,
	OpFinished,
	OpProgress,
	StatusChanged,
	SyncStateChanged,
	SyncStateKind
} from '#lib/bindings.js';

/** バックエンドのイベントから組み立てる、プロジェクトごとの実行時の状態 */
export interface LiveState {
	/** 実行中の操作名（`save` / `restore` / `pull` / `push` / `resolve`）。無ければ null */
	operation: string | null;
	sync: SyncStateKind;
	/** 要対応の理由。無ければ null */
	attention: AttentionReason | null;
}

export const initialLiveState: LiveState = { operation: null, sync: 'idle', attention: null };

export type LiveEvent =
	| { type: 'status-changed'; payload: StatusChanged }
	| { type: 'op-progress'; payload: OpProgress }
	| { type: 'op-finished'; payload: OpFinished }
	| { type: 'sync-state-changed'; payload: SyncStateChanged }
	| { type: 'needs-attention'; payload: NeedsAttention };

export interface LiveUpdate {
	state: LiveState;
	/** 該当プロジェクトの TanStack Query を無効化するか */
	invalidate: boolean;
}

/** イベントを状態へ反映する純関数。無効化の要否も一緒に返す */
export function applyLiveEvent(state: LiveState, event: LiveEvent): LiveUpdate {
	switch (event.type) {
		case 'status-changed':
			return { state, invalidate: true };
		case 'op-progress':
			return { state: { ...state, operation: event.payload.operation }, invalidate: false };
		case 'op-finished':
			return {
				state: {
					...state,
					operation: null,
					// 操作が成功したら、それまでの要対応は解消済みとみなす
					attention: event.payload.ok ? null : state.attention
				},
				invalidate: true
			};
		case 'sync-state-changed':
			return {
				state: {
					...state,
					sync: event.payload.state,
					attention: event.payload.state === 'idle' ? null : state.attention
				},
				invalidate: true
			};
		case 'needs-attention':
			return { state: { ...state, attention: event.payload.reason }, invalidate: true };
	}
}

/** ヘッダーの処理中表示に使う種別。pull は「取り込み」、push は「アップロード」、それ以外は汎用 */
export function syncingKind(operation: string | null): 'fetch' | 'push' | 'other' | null {
	if (operation === null) return null;
	if (operation === 'pull') return 'fetch';
	if (operation === 'push') return 'push';
	return 'other';
}
