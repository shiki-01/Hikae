import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';
import type { DroppedFile } from '#lib/api/types.js';
import { nameFromFolder } from '#lib/features/projects/add-project.js';

/** 画面上の位置（CSS ピクセル。`document.elementFromPoint` にそのまま渡せる） */
export interface DropPoint {
	x: number;
	y: number;
}

export type NativeDropAction =
	| { kind: 'enter'; point: DropPoint }
	| { kind: 'over'; point: DropPoint }
	| { kind: 'leave' }
	| { kind: 'drop'; files: DroppedFile[]; point: DropPoint };

/** 実パスから、追加するファイルの情報を作る（大きさは分からないため null。検査はバックエンドが行う） */
export function droppedFromPaths(paths: string[]): DroppedFile[] {
	return paths.map((path) => ({ name: nameFromFolder(path), size: null, path }));
}

/**
 * ドロップ位置（webview が返す物理ピクセル）を、画面の位置（CSS ピクセル）にする。
 * 拡大率（`window.devicePixelRatio`）で割る。拡大率が不正なら 1 として扱う。
 */
export function toLogicalPoint(position: { x: number; y: number }, scaleFactor: number): DropPoint {
	const scale = Number.isFinite(scaleFactor) && scaleFactor > 0 ? scaleFactor : 1;
	return { x: position.x / scale, y: position.y / scale };
}

/**
 * webview のドラッグ&ドロップのイベントを、画面の動作に対応づける。
 * 位置は物理ピクセルで届くため、`scaleFactor` で CSS ピクセルに換算する。
 */
export function toDropAction(payload: DragDropEvent, scaleFactor = 1): NativeDropAction | null {
	switch (payload.type) {
		case 'enter':
			return payload.paths.length > 0
				? { kind: 'enter', point: toLogicalPoint(payload.position, scaleFactor) }
				: null;
		case 'over':
			return { kind: 'over', point: toLogicalPoint(payload.position, scaleFactor) };
		case 'leave':
			return { kind: 'leave' };
		case 'drop':
			return payload.paths.length > 0
				? {
						kind: 'drop',
						files: droppedFromPaths(payload.paths),
						point: toLogicalPoint(payload.position, scaleFactor)
					}
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
		const action = toDropAction(event.payload, window.devicePixelRatio);
		if (action) handler(action);
	});
}
