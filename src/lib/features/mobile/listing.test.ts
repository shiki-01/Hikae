import { describe, expect, it } from 'vitest';
import type { MobileDirEntry } from '#lib/api/mobile-types.js';
import { sortEntries } from './listing';

function entry(name: string, kind: 'file' | 'dir'): MobileDirEntry {
	return { name, path: name, kind, size: 0, sha: name };
}

describe('フォルダの並び', () => {
	it('フォルダを先に、その中は名前順（数字は自然な並び）に並べる', () => {
		const sorted = sortEntries([
			entry('b.txt', 'file'),
			entry('z', 'dir'),
			entry('a.txt', 'file'),
			entry('a', 'dir'),
			entry('10.txt', 'file'),
			entry('9.txt', 'file')
		]);
		expect(sorted.map((item) => item.name)).toEqual([
			'a',
			'z',
			'9.txt',
			'10.txt',
			'a.txt',
			'b.txt'
		]);
	});

	it('元の配列を書き換えない', () => {
		const input = [entry('b', 'file'), entry('a', 'file')];
		sortEntries(input);
		expect(input.map((item) => item.name)).toEqual(['b', 'a']);
	});
});
