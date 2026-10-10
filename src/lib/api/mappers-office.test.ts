import { describe, it, expect } from 'vitest';
import { mapOfficeDiff } from './mappers';
import type { OfficeDiff as BackendOfficeDiff } from '#lib/bindings.js';

describe('Office 文書の比較結果の変換', () => {
	it('段落の行の種類と番号を、画面の形にする', () => {
		const result: BackendOfficeDiff = {
			kind: 'docx',
			rows: [
				{ kind: 'context', old_no: 1, new_no: 1, text: 'a' },
				{ kind: 'removed', old_no: 2, text: 'b' },
				{ kind: 'added', new_no: 2, text: 'c' },
				{ kind: 'omitted', count: 5 }
			],
			truncated: 3
		};
		expect(mapOfficeDiff(result)).toEqual({
			kind: 'docx',
			rows: [
				{ kind: 'context', oldNo: 1, newNo: 1, text: 'a' },
				{ kind: 'del', oldNo: 2, text: 'b' },
				{ kind: 'add', newNo: 2, text: 'c' },
				{ kind: 'omitted', count: 5 }
			],
			truncated: 3
		});
	});

	it('シートの差分のセルを、追加・削除の null を保ったまま変換する', () => {
		const result: BackendOfficeDiff = {
			kind: 'xlsx',
			added_sheets: ['New'],
			removed_sheets: [],
			cells: [
				{ sheet: 'Sheet1', cell: 'B3', row: 2, col: 1, before: '120', after: '150' },
				{ sheet: 'Sheet1', cell: 'C5', row: 4, col: 2, before: null, after: 'x' }
			],
			truncated: 0
		};
		expect(mapOfficeDiff(result)).toEqual({
			kind: 'xlsx',
			addedSheets: ['New'],
			removedSheets: [],
			cells: [
				{ sheet: 'Sheet1', cell: 'B3', row: 2, col: 1, before: '120', after: '150' },
				{ sheet: 'Sheet1', cell: 'C5', row: 4, col: 2, before: null, after: 'x' }
			],
			truncated: 0
		});
	});

	it('抽出できなかった理由を、画面の表記にする', () => {
		expect(mapOfficeDiff({ kind: 'unavailable', reason: 'too-large' })).toEqual({
			kind: 'unavailable',
			reason: 'too_large'
		});
		expect(mapOfficeDiff({ kind: 'unavailable', reason: 'unsupported-format' })).toEqual({
			kind: 'unavailable',
			reason: 'unsupported_format'
		});
		expect(mapOfficeDiff({ kind: 'unavailable', reason: 'corrupt' })).toEqual({
			kind: 'unavailable',
			reason: 'corrupt'
		});
	});
});
