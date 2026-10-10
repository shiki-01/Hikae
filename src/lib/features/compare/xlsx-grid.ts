// xlsx のシートの表示（SheetGrid）と、変更の重ね方（XlsxDiff）の純関数。DOM に依存しない。
// 行・列は 0 始まりで扱い、表示の英字・行番号は 1 始まりに直す。

import type { CellChange, PreviewSheet } from '#lib/api/types.js';

/** 表示するグリッドの行数の上限（超えた分は注記する） */
export const GRID_MAX_ROWS = 500;
/** 表示するグリッドの列数の上限（超えた分は注記する） */
export const GRID_MAX_COLS = 60;

/** 0 始まりの列番号を英字にする（0 → A、25 → Z、26 → AA） */
export function columnLabel(col: number): string {
	let n = col + 1;
	let label = '';
	while (n > 0) {
		const rest = (n - 1) % 26;
		label = String.fromCharCode(65 + rest) + label;
		n = Math.floor((n - 1) / 26);
	}
	return label;
}

/** 0 始まりの行・列からセル参照（A1 形式）を作る */
export function cellRef(row: number, col: number): string {
	return `${columnLabel(col)}${row + 1}`;
}

/**
 * セルの強調の種類。changed は変更（before → value）、added は追加、removed は削除（値は無く before が消えた値）。
 * 強調の無いセルは tone を持たない
 */
export type CellTone = 'changed' | 'added' | 'removed';

/** グリッドのセル 1 件（0 始まり）。value が表示する値、before は変更・削除で元の値 */
export interface GridCell {
	row: number;
	col: number;
	value: string;
	tone?: CellTone;
	before?: string | null;
}

/** シートの見出しに付けるバッジ（追加・削除されたシート） */
export type SheetBadge = 'added' | 'removed' | null;

/** 表示用のシート 1 枚。rows・cols は A1 から最後の値（強調のあるセル）までの大きさ */
export interface XlsxSheetView {
	name: string;
	rows: number;
	cols: number;
	cells: GridCell[];
	truncated: boolean;
	badge: SheetBadge;
}

/** セル位置のキー（行と列の組）。疎な配列を引くときに使う */
export function cellKey(row: number, col: number): string {
	return `${row}:${col}`;
}

/** 疎なセルの一覧を、位置から引ける Map にする */
export function toCellMap(cells: GridCell[]): Map<string, GridCell> {
	const map = new Map<string, GridCell>();
	for (const cell of cells) map.set(cellKey(cell.row, cell.col), cell);
	return map;
}

/** 表示するグリッドの大きさ。上限を超えたら `limited` にする */
export function gridSize(
	rows: number,
	cols: number
): { rows: number; cols: number; limitedRows: boolean; limitedCols: boolean } {
	return {
		rows: Math.min(rows, GRID_MAX_ROWS),
		cols: Math.min(cols, GRID_MAX_COLS),
		limitedRows: rows > GRID_MAX_ROWS,
		limitedCols: cols > GRID_MAX_COLS
	};
}

/** 強調の無い、プレビューのシート（値のあるセルをそのまま並べる） */
export function plainSheetView(sheet: PreviewSheet, badge: SheetBadge = null): XlsxSheetView {
	return {
		name: sheet.name,
		rows: sheet.rows,
		cols: sheet.cols,
		cells: sheet.cells.map((cell) => ({ row: cell.row, col: cell.col, value: cell.value })),
		truncated: sheet.truncated,
		badge
	};
}

/** 全セルを同じ強調にしたシート（シートの追加・削除、ファイルの追加・削除で使う） */
function toneAll(sheet: PreviewSheet, tone: CellTone, badge: SheetBadge): XlsxSheetView {
	const view = plainSheetView(sheet, badge);
	return {
		...view,
		// 削除されたシートの全セルは、表示する値（value）がそのまま元の値（before）になる
		cells: view.cells.map((cell) =>
			tone === 'removed' ? { ...cell, tone, before: cell.value } : { ...cell, tone }
		)
	};
}

/** 1 枚のシートに、セルの変更を重ねる */
function overlayChanges(sheet: PreviewSheet, changes: CellChange[]): XlsxSheetView {
	const map = toCellMap(plainSheetView(sheet).cells);
	let rows = sheet.rows;
	let cols = sheet.cols;
	for (const change of changes) {
		rows = Math.max(rows, change.row + 1);
		cols = Math.max(cols, change.col + 1);
		const { row, col } = change;
		if (change.before !== null && change.after !== null) {
			map.set(cellKey(row, col), {
				row,
				col,
				value: change.after,
				tone: 'changed',
				before: change.before
			});
		} else if (change.after !== null) {
			map.set(cellKey(row, col), { row, col, value: change.after, tone: 'added' });
		} else {
			// 削除: 今の版には値が無いので、空のセルを赤系で出し、元の値を before に持つ
			map.set(cellKey(row, col), {
				row,
				col,
				value: '',
				tone: 'removed',
				before: change.before ?? ''
			});
		}
	}
	return {
		name: sheet.name,
		rows,
		cols,
		cells: [...map.values()],
		truncated: sheet.truncated,
		badge: null
	};
}

/** xlsx の比較の入力。`to` は今の版（無ければ null）、`from` は過去の版（無ければ null） */
export interface XlsxDiffInput {
	to: PreviewSheet[] | null;
	from: PreviewSheet[] | null;
	addedSheets: string[];
	removedSheets: string[];
	cells: CellChange[];
}

/**
 * 今の版のシートを土台に、セルの変更を重ねる。
 * - 今の版が無い（削除）ときは、過去の版のシートの全セルを「削除」にする
 * - 過去の版が無い（新規）ときは、今の版の全セルを「追加」にする
 * - 追加されたシートは全セルを「追加」にし、削除されたシートは過去の版のグリッドを「削除」で並べる（タブの後ろに付く）
 */
export function mergeXlsxDiff(input: XlsxDiffInput): XlsxSheetView[] {
	const { to, from, addedSheets, removedSheets, cells } = input;
	if (to === null) {
		return (from ?? []).map((sheet) => toneAll(sheet, 'removed', null));
	}
	if (from === null) {
		return to.map((sheet) => toneAll(sheet, 'added', null));
	}
	const added = new Set(addedSheets);
	const removed = new Set(removedSheets);
	const views: XlsxSheetView[] = [];
	for (const sheet of to) {
		if (added.has(sheet.name)) {
			views.push(toneAll(sheet, 'added', 'added'));
		} else {
			views.push(
				overlayChanges(
					sheet,
					cells.filter((cell) => cell.sheet === sheet.name)
				)
			);
		}
	}
	for (const sheet of from) {
		if (removed.has(sheet.name)) views.push(toneAll(sheet, 'removed', 'removed'));
	}
	return views;
}
