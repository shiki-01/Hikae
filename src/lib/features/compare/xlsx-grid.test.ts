import { describe, it, expect } from 'vitest';
import type { CellChange, PreviewSheet } from '#lib/api/types.js';
import {
	cellKey,
	cellRef,
	columnLabel,
	gridSize,
	mergeXlsxDiff,
	toCellMap,
	GRID_MAX_COLS,
	GRID_MAX_ROWS
} from './xlsx-grid';

function sheet(
	name: string,
	rows: number,
	cols: number,
	cells: [number, number, string][]
): PreviewSheet {
	return {
		name,
		rows,
		cols,
		cells: cells.map(([row, col, value]) => ({ row, col, value })),
		truncated: false
	};
}

function change(
	sheetName: string,
	row: number,
	col: number,
	before: string | null,
	after: string | null
): CellChange {
	return { sheet: sheetName, cell: cellRef(row, col), row, col, before, after };
}

describe('columnLabel', () => {
	it('0 始まりの列番号を英字にする', () => {
		expect(columnLabel(0)).toBe('A');
		expect(columnLabel(25)).toBe('Z');
		expect(columnLabel(26)).toBe('AA');
		expect(columnLabel(27)).toBe('AB');
		expect(columnLabel(701)).toBe('ZZ');
		expect(columnLabel(702)).toBe('AAA');
	});

	it('セル参照は行を 1 始まりにする', () => {
		expect(cellRef(2, 1)).toBe('B3');
		expect(cellRef(0, 26)).toBe('AA1');
	});
});

describe('疎なセルの参照', () => {
	it('位置のキーで値を引ける', () => {
		const map = toCellMap([{ row: 1, col: 2, value: 'x' }]);
		expect(map.get(cellKey(1, 2))?.value).toBe('x');
		expect(map.get(cellKey(0, 0))).toBeUndefined();
	});
});

describe('gridSize', () => {
	it('上限以内はそのまま、超えたら上限に切り、注記する', () => {
		expect(gridSize(10, 3)).toEqual({ rows: 10, cols: 3, limitedRows: false, limitedCols: false });
		expect(gridSize(GRID_MAX_ROWS + 1, 3)).toEqual({
			rows: GRID_MAX_ROWS,
			cols: 3,
			limitedRows: true,
			limitedCols: false
		});
		expect(gridSize(2, GRID_MAX_COLS + 5)).toEqual({
			rows: 2,
			cols: GRID_MAX_COLS,
			limitedRows: false,
			limitedCols: true
		});
	});
});

describe('mergeXlsxDiff', () => {
	const before = [
		sheet('Sheet1', 4, 3, [
			[0, 0, '項目'],
			[2, 1, '120']
		]),
		sheet('Sheet2', 1, 1, [[0, 0, '廃止']])
	];
	const after = [
		sheet('Sheet1', 4, 3, [
			[0, 0, '項目'],
			[2, 1, '150'],
			[4, 2, '新しい値']
		]),
		sheet('Sheet3', 1, 1, [[0, 0, '追加']])
	];

	it('変更は changed、追加は added、削除は空のセルの removed として重ねる', () => {
		const views = mergeXlsxDiff({
			to: after,
			from: before,
			addedSheets: ['Sheet3'],
			removedSheets: ['Sheet2'],
			cells: [change('Sheet1', 2, 1, '120', '150'), change('Sheet1', 4, 2, null, '新しい値')]
		});
		const sheet1 = views.find((v) => v.name === 'Sheet1');
		const map = toCellMap(sheet1?.cells ?? []);
		expect(map.get(cellKey(2, 1))).toEqual({
			row: 2,
			col: 1,
			value: '150',
			tone: 'changed',
			before: '120'
		});
		expect(map.get(cellKey(4, 2))?.tone).toBe('added');
		expect(map.get(cellKey(0, 0))?.tone).toBeUndefined();
		expect(sheet1?.badge).toBeNull();
	});

	it('削除のセルは、今の版では空にして before に元の値を持つ', () => {
		const views = mergeXlsxDiff({
			to: [sheet('Sheet1', 3, 2, [[0, 0, '項目']])],
			from: [
				sheet('Sheet1', 3, 2, [
					[0, 0, '項目'],
					[1, 1, '旧']
				])
			],
			addedSheets: [],
			removedSheets: [],
			cells: [change('Sheet1', 1, 1, '旧', null)]
		});
		const removed = toCellMap(views[0].cells).get(cellKey(1, 1));
		expect(removed).toEqual({ row: 1, col: 1, value: '', tone: 'removed', before: '旧' });
	});

	it('追加されたシートは全セルが追加で、タブに追加のバッジが付く。削除されたシートは過去の版で後ろに並ぶ', () => {
		const views = mergeXlsxDiff({
			to: after,
			from: before,
			addedSheets: ['Sheet3'],
			removedSheets: ['Sheet2'],
			cells: []
		});
		expect(views.map((v) => [v.name, v.badge])).toEqual([
			['Sheet1', null],
			['Sheet3', 'added'],
			['Sheet2', 'removed']
		]);
		expect(views[1].cells.every((c) => c.tone === 'added')).toBe(true);
		expect(views[2].cells).toEqual([
			{ row: 0, col: 0, value: '廃止', tone: 'removed', before: '廃止' }
		]);
	});

	it('今の版が無い（削除）ときは過去の版の全セルを削除にする', () => {
		const views = mergeXlsxDiff({
			to: null,
			from: before,
			addedSheets: [],
			removedSheets: [],
			cells: []
		});
		expect(views).toHaveLength(2);
		expect(views[0].cells.every((c) => c.tone === 'removed')).toBe(true);
	});

	it('過去の版が無い（新規）ときは今の版の全セルを追加にする', () => {
		const views = mergeXlsxDiff({
			to: after,
			from: null,
			addedSheets: [],
			removedSheets: [],
			cells: []
		});
		expect(views.map((v) => v.name)).toEqual(['Sheet1', 'Sheet3']);
		expect(views[0].cells.every((c) => c.tone === 'added')).toBe(true);
	});

	it('両方とも無ければ、表示するものは無い', () => {
		expect(
			mergeXlsxDiff({ to: null, from: null, addedSheets: [], removedSheets: [], cells: [] })
		).toEqual([]);
	});

	it('変更がセルの範囲の外にあるときは、表示する大きさを広げる', () => {
		const views = mergeXlsxDiff({
			to: [sheet('Sheet1', 2, 2, [])],
			from: [sheet('Sheet1', 2, 2, [])],
			addedSheets: [],
			removedSheets: [],
			cells: [change('Sheet1', 5, 4, null, 'x')]
		});
		expect(views[0].rows).toBe(6);
		expect(views[0].cols).toBe(5);
	});
});
