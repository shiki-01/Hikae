import type { SavePoint } from '#lib/api/types.js';

/** 履歴を 1 回に取得する件数（無限スクロールの 1 ページ） */
export const HISTORY_PAGE_SIZE = 100;

/**
 * 取得済みのページをつなげて 1 本の履歴にする。
 * ページを取っている間に保存が増えると、境目の項目が次のページにも現れることがあるため、
 * 同じ ID は先に出たものだけを残す。
 */
export function flattenHistoryPages(pages: SavePoint[][]): SavePoint[] {
	const seen = new Set<string>();
	const result: SavePoint[] = [];
	for (const page of pages) {
		for (const point of page) {
			if (seen.has(point.id)) continue;
			seen.add(point.id);
			result.push(point);
		}
	}
	return result;
}

/**
 * 次のページの開始位置。最後のページが満たなければ、これ以上は無いので undefined。
 * 位置は重複を除く前の取得件数で数える（バックエンドの並びの位置と一致させるため）。
 */
export function nextHistoryOffset(
	pages: SavePoint[][],
	pageSize: number = HISTORY_PAGE_SIZE
): number | undefined {
	const last = pages[pages.length - 1];
	if (!last || last.length < pageSize) return undefined;
	return pages.reduce((sum, page) => sum + page.length, 0);
}
