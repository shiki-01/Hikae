import { describe, it, expect } from 'vitest';
import type { NameConflict } from '#lib/api/types.js';
import {
	allSkipped,
	applyToAll,
	choose,
	commonChoice,
	defaultChoices,
	splitPath,
	toDecisions
} from './add-conflict';

const conflicts: NameConflict[] = [
	{ path: 'a.txt', canReplace: true },
	{ path: '資料/b.psd', canReplace: false },
	{ path: '資料/c.docx', canReplace: true }
];

describe('同名のファイルの選択', () => {
	it('最初は、上書きしない「両方残す」にそろえる', () => {
		expect(defaultChoices(conflicts)).toEqual({
			'a.txt': 'keep_both',
			'資料/b.psd': 'keep_both',
			'資料/c.docx': 'keep_both'
		});
	});

	it('すべてに適用するとき、置き換えられないものは両方残すにする', () => {
		expect(applyToAll(conflicts, 'replace')).toEqual({
			'a.txt': 'replace',
			'資料/b.psd': 'keep_both',
			'資料/c.docx': 'replace'
		});
		expect(applyToAll(conflicts, 'skip')).toEqual({
			'a.txt': 'skip',
			'資料/b.psd': 'skip',
			'資料/c.docx': 'skip'
		});
	});

	it('ファイルごとに選べるが、置き換えられないものは置き換えにしない', () => {
		const start = defaultChoices(conflicts);
		const next = choose(start, conflicts, 'a.txt', 'replace');
		expect(next['a.txt']).toBe('replace');
		expect(choose(next, conflicts, '資料/b.psd', 'replace')).toBe(next);
		expect(choose(next, conflicts, 'no-such.txt', 'skip')).toBe(next);
		expect(choose(next, conflicts, '資料/b.psd', 'skip')['資料/b.psd']).toBe('skip');
	});

	it('送る選択は、一覧のすべてのファイルについて作る（未選択は両方残す）', () => {
		expect(toDecisions(conflicts, { 'a.txt': 'replace', '資料/c.docx': 'skip' })).toEqual([
			{ path: 'a.txt', action: 'replace' },
			{ path: '資料/b.psd', action: 'keep_both' },
			{ path: '資料/c.docx', action: 'skip' }
		]);
	});

	it('全部が同じ選択か、ばらばらかを判定する', () => {
		expect(commonChoice(conflicts, applyToAll(conflicts, 'keep_both'))).toBe('keep_both');
		expect(commonChoice(conflicts, { 'a.txt': 'replace' })).toBeNull();
		expect(commonChoice([], {})).toBeNull();
		expect(allSkipped(conflicts, applyToAll(conflicts, 'skip'))).toBe(true);
		expect(allSkipped(conflicts, defaultChoices(conflicts))).toBe(false);
	});

	it('パスをフォルダとファイル名に分ける', () => {
		expect(splitPath('a.txt')).toEqual({ name: 'a.txt', folder: '' });
		expect(splitPath('資料/図/b.psd')).toEqual({ name: 'b.psd', folder: '資料/図' });
	});
});
