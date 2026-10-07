import { describe, it, expect } from 'vitest';
import type { DiffRow } from '#lib/api/types.js';
import { annotateBlocks, countBlocks, pairRows } from './diff-rows';

function row(kind: DiffRow['kind'], text: string): DiffRow {
	return { kind, oldNo: kind === 'add' ? null : 1, newNo: kind === 'del' ? null : 1, text };
}

describe('差分行の整理', () => {
	const rows = [
		row('context', 'a'),
		row('del', 'b'),
		row('add', 'B'),
		row('add', 'B2'),
		row('context', 'c'),
		row('add', 'd')
	];

	it('連続する変更行は1つの差分箇所として数える', () => {
		expect(countBlocks(rows)).toBe(2);
		expect(annotateBlocks(rows).map((r) => r.block)).toEqual([null, 0, 0, 0, null, 1]);
	});

	it('変更が無ければ0箇所', () => {
		expect(countBlocks([row('context', 'a')])).toBe(0);
		expect(countBlocks([])).toBe(0);
	});

	it('並べる表示では削除行と追加行が左右に対応づく', () => {
		const paired = pairRows(rows);
		expect(paired).toHaveLength(5);
		expect(paired[1].left?.text).toBe('b');
		expect(paired[1].right?.text).toBe('B');
		expect(paired[2].left).toBeNull();
		expect(paired[2].right?.text).toBe('B2');
		expect(paired[0].left).toBe(paired[0].right);
	});

	it('追加のみ・削除のみの箇所は片側だけが埋まる', () => {
		const paired = pairRows([row('del', 'x'), row('context', 'y'), row('add', 'z')]);
		expect(paired[0].right).toBeNull();
		expect(paired[2].left).toBeNull();
	});
});
