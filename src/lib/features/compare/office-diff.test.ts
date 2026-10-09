import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import type { OfficeDiff } from '#lib/api/types.js';
import {
	describeCellChange,
	groupCellChanges,
	hasOfficeChanges,
	officeKindOf,
	unavailableMessageKey
} from './office-diff';

describe('officeKindOf', () => {
	it('docx と xlsx を、大文字小文字に関わらず拡張子で判定する', () => {
		expect(officeKindOf('資料/第3章.docx')).toBe('docx');
		expect(officeKindOf('dir\\Book.XLSX')).toBe('xlsx');
		expect(officeKindOf('a.Docx')).toBe('docx');
	});

	it('対応しない拡張子と、拡張子の形にならないものは対象にしない', () => {
		expect(officeKindOf('old.doc')).toBeNull();
		expect(officeKindOf('macro.docm')).toBeNull();
		expect(officeKindOf('data.xls')).toBeNull();
		expect(officeKindOf('pic.png')).toBeNull();
		expect(officeKindOf('noext')).toBeNull();
		expect(officeKindOf('.docx')).toBeNull();
		expect(officeKindOf('dir.docx/x')).toBeNull();
	});
});

describe('describeCellChange', () => {
	it('変更は「Sheet1!B3: 120 → 150」の形にする', () => {
		const line = describeCellChange({
			sheet: 'Sheet1',
			cell: 'B3',
			before: '120',
			after: '150'
		});
		expect(t(line.key, line.params)).toBe('Sheet1!B3: 120 → 150');
	});

	it('追加は「(なし) → 値」ではなく「追加: 値」、削除は「削除: 値」にする', () => {
		const added = describeCellChange({
			sheet: 'Sheet2',
			cell: 'C5',
			before: null,
			after: '新しい値'
		});
		expect(t(added.key, added.params)).toBe('Sheet2!C5: 追加: 新しい値');
		const removed = describeCellChange({ sheet: 'Sheet1', cell: 'A1', before: '旧', after: null });
		expect(t(removed.key, removed.params)).toBe('Sheet1!A1: 削除: 旧');
	});
});

describe('groupCellChanges', () => {
	it('シートの出現順を保ち、同じシートの行を 1 つにまとめる', () => {
		const groups = groupCellChanges([
			{ sheet: 'B', cell: 'A1', before: '1', after: '2' },
			{ sheet: 'A', cell: 'A2', before: null, after: 'x' },
			{ sheet: 'B', cell: 'C1', before: 'y', after: null }
		]);
		expect(groups.map((g) => g.sheet)).toEqual(['B', 'A']);
		expect(groups[0].changes.map((c) => c.cell)).toEqual(['A1', 'C1']);
		expect(groups[1].changes.map((c) => c.cell)).toEqual(['A2']);
	});

	it('空の一覧は空のグループになる', () => {
		expect(groupCellChanges([])).toEqual([]);
	});
});

describe('hasOfficeChanges', () => {
	it('段落は、省略の行しかなければ違いは無い', () => {
		const same: OfficeDiff = { kind: 'docx', rows: [{ kind: 'omitted', count: 12 }], truncated: 0 };
		expect(hasOfficeChanges(same)).toBe(false);
		const changed: OfficeDiff = {
			kind: 'docx',
			rows: [
				{ kind: 'omitted', count: 2 },
				{ kind: 'add', newNo: 3, text: '追加した段落' }
			],
			truncated: 0
		};
		expect(hasOfficeChanges(changed)).toBe(true);
	});

	it('シートは、追加・削除・セルの差分のどれかがあれば違いがある', () => {
		const empty: OfficeDiff = {
			kind: 'xlsx',
			addedSheets: [],
			removedSheets: [],
			cells: [],
			truncated: 0
		};
		expect(hasOfficeChanges(empty)).toBe(false);
		expect(hasOfficeChanges({ ...empty, addedSheets: ['New'] })).toBe(true);
		expect(hasOfficeChanges({ ...empty, removedSheets: ['Old'] })).toBe(true);
		expect(
			hasOfficeChanges({
				...empty,
				cells: [{ sheet: 'Sheet1', cell: 'B3', before: '1', after: '2' }]
			})
		).toBe(true);
	});

	it('抽出できなかったものは、違いの有無を判定しない（false）', () => {
		expect(hasOfficeChanges({ kind: 'unavailable', reason: 'corrupt' })).toBe(false);
	});
});

describe('unavailableMessageKey', () => {
	it('理由ごとに、定義済みの文言のキーを返す', () => {
		for (const reason of ['too_large', 'unsupported_format', 'corrupt'] as const) {
			expect(t(unavailableMessageKey(reason)).length).toBeGreaterThan(0);
		}
	});
});
