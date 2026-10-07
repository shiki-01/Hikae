import { describe, it, expect } from 'vitest';
import type { Change } from '#lib/api/types.js';
import { classifyDropped, MAX_FILE_BYTES } from './files';
import { suggestMemo } from './memo';

function change(path: string, type: Change['type']): Change {
	return { id: path, path, type, isConflict: false };
}

describe('ルールベースのメモ提案', () => {
	it('変更が無ければ空', () => {
		expect(suggestMemo([])).toBe('');
	});

	it('種別ごとの文言をファイル名つきで並べる', () => {
		const memo = suggestMemo([
			change('a/chapter.docx', 'modified'),
			change('fig.png', 'added'),
			change('old.txt', 'deleted')
		]);
		expect(memo).toContain('chapter.docx');
		expect(memo).toContain('fig.png');
		expect(memo).toContain('old.txt');
		expect(memo).not.toContain('a/');
	});

	it('4件以上は先頭3件と残りの件数にまとめる', () => {
		const memo = suggestMemo([
			change('1.txt', 'modified'),
			change('2.txt', 'modified'),
			change('3.txt', 'modified'),
			change('4.txt', 'modified'),
			change('5.txt', 'added')
		]);
		expect(memo).toContain('3.txt');
		expect(memo).not.toContain('4.txt');
		expect(memo).toContain('2');
	});
});

describe('追加するファイルの大きさの判定', () => {
	it('100MB を超えるファイルだけを受け付けない', () => {
		const result = classifyDropped([
			{ name: 'ok.txt', size: 10 },
			{ name: 'edge.bin', size: MAX_FILE_BYTES },
			{ name: 'big.mp4', size: MAX_FILE_BYTES + 1 }
		]);
		expect(result.accepted.map((f) => f.name)).toEqual(['ok.txt', 'edge.bin']);
		expect(result.rejected.map((f) => f.name)).toEqual(['big.mp4']);
	});
});
