import { describe, it, expect } from 'vitest';
import {
	composeResolved,
	hunkCount,
	initialPicks,
	parseConflictMarkers,
	pickText,
	setPick,
	type HunkPick
} from './conflict-markers';

describe('競合マーカーの解析', () => {
	it('CRLF のファイルは改行コードを保ったまま解析し、CRLF で組み立てる', () => {
		const text = 'a\r\n<<<<<<< HEAD\r\nm1\r\n=======\r\nt1\r\n>>>>>>> b\r\nz\r\n';
		const doc = parseConflictMarkers(text);
		expect(doc).not.toBeNull();
		if (!doc) return;
		expect(doc.eol).toBe('\r\n');
		expect(doc.segments).toEqual([
			{ kind: 'common', text: 'a\r\n' },
			{ kind: 'hunk', mine: 'm1\r\n', base: null, theirs: 't1\r\n' },
			{ kind: 'common', text: 'z\r\n' }
		]);
		expect(composeResolved(doc, [{ kind: 'mine' }])).toBe('a\r\nm1\r\nz\r\n');
		expect(composeResolved(doc, [{ kind: 'theirs' }])).toBe('a\r\nt1\r\nz\r\n');
		expect(composeResolved(doc, [{ kind: 'both' }])).toBe('a\r\nm1\r\nt1\r\nz\r\n');
	});

	it('複数のハンクを順に解析し、ハンクごとに選べる', () => {
		const text = 'x\n<<<<<<< H\nA\n=======\nB\n>>>>>>> c\ny\n<<<<<<< H\nC\n=======\nD\n>>>>>>> c\n';
		const doc = parseConflictMarkers(text);
		expect(doc).not.toBeNull();
		if (!doc) return;
		expect(hunkCount(doc)).toBe(2);
		expect(composeResolved(doc, [{ kind: 'theirs' }, { kind: 'mine' }])).toBe('x\nB\ny\nC\n');
		expect(composeResolved(doc, [{ kind: 'mine' }, { kind: 'theirs' }])).toBe('x\nA\ny\nD\n');
	});

	it('diff3 形式の基底部分は読み飛ばして base に保持し、組み立てには含めない', () => {
		const text = '<<<<<<< HEAD\nm\n||||||| merged common ancestors\nbase\n=======\nt\n>>>>>>> b\n';
		const doc = parseConflictMarkers(text);
		expect(doc).not.toBeNull();
		if (!doc) return;
		expect(doc.segments).toEqual([{ kind: 'hunk', mine: 'm\n', base: 'base\n', theirs: 't\n' }]);
		expect(composeResolved(doc, [{ kind: 'mine' }])).toBe('m\n');
	});

	it('「両方」は、この PC の版の後にクラウドの版を置く', () => {
		const text = 'p\n<<<<<<< H\nm\n=======\nt\n>>>>>>> c\nq\n';
		const doc = parseConflictMarkers(text);
		if (!doc) throw new Error('parse failed');
		expect(composeResolved(doc, [{ kind: 'both' }])).toBe('p\nm\nt\nq\n');
	});

	it('末尾改行が無いファイルの境界を保つ', () => {
		// 最後の共通部分に改行が無い
		const withTail = parseConflictMarkers('pre\n<<<<<<< H\nm\n=======\nt\n>>>>>>> c\nlast');
		expect(withTail).not.toBeNull();
		if (!withTail) return;
		expect(composeResolved(withTail, [{ kind: 'mine' }])).toBe('pre\nm\nlast');
		expect(composeResolved(withTail, [{ kind: 'both' }])).toBe('pre\nm\nt\nlast');

		// 最後の `>>>>>>>` の行にも改行が無い
		const noEolAtEnd = parseConflictMarkers('pre\n<<<<<<< H\nm\n=======\nt\n>>>>>>> c');
		expect(noEolAtEnd).not.toBeNull();
		if (!noEolAtEnd) return;
		expect(composeResolved(noEolAtEnd, [{ kind: 'theirs' }])).toBe('pre\nt\n');
	});

	it('解析できない入力は null を返す', () => {
		// マーカーが無い
		expect(parseConflictMarkers('plain\n')).toBeNull();
		// 閉じ忘れ（`>>>>>>>` が無い）
		expect(parseConflictMarkers('<<<<<<< H\nm\n=======\nt\n')).toBeNull();
		// `=======` が無い
		expect(parseConflictMarkers('<<<<<<< H\nm\n>>>>>>> c\n')).toBeNull();
		// 競合の外に `>>>>>>>` がある（順序不正）
		expect(parseConflictMarkers('a\n>>>>>>> c\n')).toBeNull();
		// 入れ子
		expect(parseConflictMarkers('<<<<<<< H\n<<<<<<< X\nm\n=======\nt\n>>>>>>> c\n')).toBeNull();
		// `>>>>>>>` の後に `=======` が来る（順序不正）
		expect(parseConflictMarkers('<<<<<<< H\nm\n>>>>>>> c\n=======\n')).toBeNull();
	});

	it('競合の外の `=======` は通常の文字として扱う', () => {
		const text = '=======\n<<<<<<< H\nm\n=======\nt\n>>>>>>> c\n';
		const doc = parseConflictMarkers(text);
		expect(doc).not.toBeNull();
		if (!doc) return;
		expect(doc.segments[0]).toEqual({ kind: 'common', text: '=======\n' });
		expect(composeResolved(doc, [{ kind: 'mine' }])).toBe('=======\nm\n');
	});
});

describe('ハンクの選択と組み立て', () => {
	const doc = parseConflictMarkers('x\n<<<<<<< H\nA\n=======\nB\n>>>>>>> c\ny\n');
	if (!doc) throw new Error('parse failed');

	it('選択が足りないときは組み立てない', () => {
		expect(composeResolved(doc, [])).toBeNull();
		expect(composeResolved(doc, [null])).toBeNull();
	});

	it('選択の更新は他のハンクに影響しない', () => {
		const picks = setPick(initialPicks(doc), 0, { kind: 'theirs' });
		expect(picks).toEqual([{ kind: 'theirs' }]);
		expect(setPick(picks, 0, null)).toEqual([null]);
	});

	it('編集したテキストが改行で終わらず後ろに続きがあるときは、改行を補う', () => {
		const twoHunks = parseConflictMarkers(
			'x\n<<<<<<< H\nA\n=======\nB\n>>>>>>> c\ny\n<<<<<<< H\nC\n=======\nD\n>>>>>>> c\n'
		);
		if (!twoHunks) throw new Error('parse failed');
		const edited: HunkPick = { kind: 'edited', text: 'E' };
		expect(composeResolved(twoHunks, [edited, { kind: 'theirs' }])).toBe('x\nE\ny\nD\n');
		// 最後のハンクなら補わない（元のファイルと同じく末尾改行は付けない）
		expect(composeResolved(twoHunks, [{ kind: 'mine' }, edited])).toBe('x\nA\ny\nE');
	});

	it('pickText は選択の版のテキストを返す', () => {
		const hunk = doc.segments[1];
		if (hunk.kind !== 'hunk') throw new Error('expected a hunk');
		expect(pickText(hunk, { kind: 'mine' })).toBe('A\n');
		expect(pickText(hunk, { kind: 'theirs' })).toBe('B\n');
		expect(pickText(hunk, { kind: 'both' })).toBe('A\nB\n');
		expect(pickText(hunk, { kind: 'edited', text: 'Z' })).toBe('Z');
	});
});
