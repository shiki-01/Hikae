// Office 文書（docx・xlsx）の比較の画面用の整形（設計書 3.3 S4）。DOM に依存しない純関数。
// 文言は i18n のキーと引数で返し、組み立ては呼び出し側（t）が行う。

import type { MessageKey } from '#lib/i18n/index.js';

import type { CellChange, OfficeDiff } from '#lib/api/types.js';

/** 比べる対象の Office 文書の種類 */
export type OfficeKindName = 'docx' | 'xlsx';

/**
 * パスの拡張子から種類を決める（大文字小文字は区別しない）。
 * `core-preview` の `office_kind` と同じ規則で、名前が `.docx` だけのものや、
 * 拡張子の後ろにパス区切りが続くものは対象にしない。
 */
export function officeKindOf(path: string): OfficeKindName | null {
	const name = path.split(/[\\/]/).pop() ?? '';
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return null;
	const ext = name.slice(dot + 1).toLowerCase();
	return ext === 'docx' || ext === 'xlsx' ? ext : null;
}

/** 画面の表記のセル参照（「Sheet1!B3」） */
export function cellRefOf(change: CellChange): string {
	return `${change.sheet}!${change.cell}`;
}

/** セルの差分 1 件の文言のキーと引数（変更・追加・削除で文言を分ける） */
export interface CellLine {
	key: MessageKey;
	params: Record<string, string>;
}

/**
 * セルの差分 1 件の表示。変更は「Sheet1!B3: 120 → 150」、追加は「Sheet1!C5: 追加: 値」、
 * 削除は「Sheet1!A1: 削除: 値」にする（追加・削除で「(なし) → 値」は使わない）
 */
export function describeCellChange(change: CellChange): CellLine {
	const ref = cellRefOf(change);
	if (change.before !== null && change.after !== null) {
		return {
			key: 'compare.office.cell_changed',
			params: { ref, before: change.before, after: change.after }
		};
	}
	if (change.after !== null) {
		return { key: 'compare.office.cell_added', params: { ref, value: change.after } };
	}
	return { key: 'compare.office.cell_removed', params: { ref, value: change.before ?? '' } };
}

/** シートごとのセルの差分（シートの出現順を保つ） */
export interface SheetGroup {
	sheet: string;
	changes: CellChange[];
}

/** セルの差分をシート名でまとめる。同じシートの行は、元の順（行 → 列）を保つ */
export function groupCellChanges(cells: CellChange[]): SheetGroup[] {
	const groups: SheetGroup[] = [];
	const index = new Map<string, SheetGroup>();
	for (const change of cells) {
		let group = index.get(change.sheet);
		if (!group) {
			group = { sheet: change.sheet, changes: [] };
			index.set(change.sheet, group);
			groups.push(group);
		}
		group.changes.push(change);
	}
	return groups;
}

/** 差分に内容の違いがあるか（変更のない段落の省略だけなら、違いは無い） */
export function hasOfficeChanges(diff: OfficeDiff): boolean {
	switch (diff.kind) {
		case 'docx':
			return diff.rows.some((row) => row.kind !== 'omitted');
		case 'xlsx':
			return diff.addedSheets.length > 0 || diff.removedSheets.length > 0 || diff.cells.length > 0;
		case 'unavailable':
			return false;
	}
}
