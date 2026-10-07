import { describe, it, expect } from 'vitest';
import { diffLines } from './diff-lines';

describe('diffLines', () => {
	it('同一の行列はすべて context になる', () => {
		const rows = diffLines(['a', 'b'], ['a', 'b']);
		expect(rows.map((r) => r.kind)).toEqual(['context', 'context']);
	});

	it('置換は削除行のあとに追加行が並ぶ', () => {
		const rows = diffLines(['a', 'b', 'c'], ['a', 'x', 'c']);
		expect(rows.map((r) => r.kind)).toEqual(['context', 'del', 'add', 'context']);
		expect(rows[1]).toMatchObject({ oldNo: 2, newNo: null, text: 'b' });
		expect(rows[2]).toMatchObject({ oldNo: null, newNo: 2, text: 'x' });
	});

	it('末尾への追加と先頭の削除を扱える', () => {
		const rows = diffLines(['a', 'b'], ['b', 'c']);
		expect(rows.map((r) => r.kind)).toEqual(['del', 'context', 'add']);
	});

	it('空配列との比較ができる', () => {
		expect(diffLines([], ['a']).map((r) => r.kind)).toEqual(['add']);
		expect(diffLines(['a'], []).map((r) => r.kind)).toEqual(['del']);
		expect(diffLines([], [])).toEqual([]);
	});
});
