import { describe, it, expect } from 'vitest';
import type { Change } from '#lib/api/types.js';
import { classifyDropped, MAX_FILE_BYTES } from './files';
import { nextMemo, suggestMemo } from './memo';

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

describe('保存欄の初期入力', () => {
	it('空欄には案を入れる', () => {
		expect(nextMemo('', '', 'A を更新')).toBe('A を更新');
	});

	it('自動で入れた案のままなら新しい案に差し替える', () => {
		expect(nextMemo('A を更新', 'A を更新', 'A ほか 1 件')).toBe('A ほか 1 件');
	});

	it('ユーザーが編集した内容は上書きしない', () => {
		expect(nextMemo('自分のメモ', 'A を更新', 'A ほか 1 件')).toBe('自分のメモ');
	});

	it('変更が無くなったら自動入力分は消える', () => {
		expect(nextMemo('A を更新', 'A を更新', '')).toBe('');
	});
});
