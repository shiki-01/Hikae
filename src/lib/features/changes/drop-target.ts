import type { DropPoint } from './native-drop';

/** ツリーのフォルダ行に付ける属性。値はそのフォルダのパス（プロジェクトからの相対パス） */
export const DROP_FOLDER_ATTR = 'data-drop-folder';

/** 位置から要素を引く窓口（`document` がそのまま使える。テストでは差し替える） */
export interface HitTest {
	elementFromPoint(x: number, y: number): Element | null;
}

/**
 * ドロップ位置にあるフォルダ行のパスを返す。追加先の決定に使う純関数。
 *
 * - ツリー表示（「すべてのファイル」）でないときは、常に null（プロジェクト直下）
 * - フォルダ行の上だけがそのフォルダ。ファイルの行・ツリーの余白・ほかの場所は null（プロジェクト直下）
 */
export function folderAtPoint(
	point: DropPoint,
	view: HitTest | null,
	treeActive: boolean
): string | null {
	if (!treeActive || view === null) return null;
	const element = view.elementFromPoint(point.x, point.y);
	const row = element?.closest(`[${DROP_FOLDER_ATTR}]`);
	const path = row?.getAttribute(DROP_FOLDER_ATTR);
	return path ? path : null;
}

/** フォルダのパスの最後の名前（画面に出す名前） */
export function folderLabel(path: string): string {
	return path.split('/').filter(Boolean).pop() ?? path;
}
