import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';
import type { DroppedFile } from '#lib/api/types.js';
import { nameFromFolder } from '#lib/features/projects/add-project.js';

export type NativeDropAction =
	{ kind: 'enter' } | { kind: 'leave' } | { kind: 'drop'; files: DroppedFile[] };

/** 実パスから、追加するファイルの情報を作る（大きさは分からないため null。検査はバックエンドが行う） */
export function droppedFromPaths(paths: string[]): DroppedFile[] {
	return paths.map((path) => ({ name: nameFromFolder(path), size: null, path }));
}

/** webview のドラッグ&ドロップのイベントを、画面の動作に対応づける。移動中（over）は何もしない */
export function toDropAction(payload: DragDropEvent): NativeDropAction | null {
	switch (payload.type) {
		case 'enter':
			return payload.paths.length > 0 ? { kind: 'enter' } : null;
		case 'leave':
			return { kind: 'leave' };
		case 'drop':
			return payload.paths.length > 0
				? { kind: 'drop', files: droppedFromPaths(payload.paths) }
				: { kind: 'leave' };
		default:
			return null;
	}
}

/**
 * アプリ上では、ファイルのドロップを webview が受け取り、実パスが得られる（HTML のドロップは発生しない）。
 * 戻り値は購読の解除関数。
 */
export function listenNativeDrop(handler: (action: NativeDropAction) => void): Promise<() => void> {
	return getCurrentWebview().onDragDropEvent((event) => {
		const action = toDropAction(event.payload);
		if (action) handler(action);
	});
}
