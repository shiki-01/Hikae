import { describe, expect, it } from 'vitest';
import {
	clipboardFileName,
	memoFileName,
	splitExtension,
	timestampStamp,
	uniqueName
} from './names';

const now = new Date(2026, 9, 5, 7, 3); // 2026-10-05 07:03（端末の時刻）

describe('既定の名前', () => {
	it('日時は YYYY-MM-DD_HHmm（端末の時刻）', () => {
		expect(timestampStamp(now)).toBe('2026-10-05_0703');
	});

	it('メモの既定名は「メモ_日時.txt」。同じ名前があれば (2) を付ける', () => {
		expect(memoFileName(now, new Set())).toBe('メモ_2026-10-05_0703.txt');
		expect(memoFileName(now, new Set(['メモ_2026-10-05_0703.txt']))).toBe(
			'メモ_2026-10-05_0703 (2).txt'
		);
	});

	it('クリップボードの既定名は「クリップボード_日時.拡張子」', () => {
		expect(clipboardFileName(now, 'png', new Set())).toBe('クリップボード_2026-10-05_0703.png');
	});
});

describe('重複しない名前', () => {
	it('拡張子を前後に分ける（拡張子が無ければ空）', () => {
		expect(splitExtension('x.tar.gz')).toEqual({ stem: 'x.tar', ext: 'gz' });
		expect(splitExtension('README')).toEqual({ stem: 'README', ext: '' });
	});

	it('重ならなければそのまま返す', () => {
		expect(uniqueName('a.txt', new Set(['b.txt']))).toBe('a.txt');
	});

	it('大文字と小文字を区別せずに重複を見る', () => {
		expect(uniqueName('README.md', new Set(['readme.md']))).toBe('README (2).md');
	});

	it('既存の (2) も避けて、空いている番号を使う', () => {
		expect(uniqueName('a.txt', new Set(['a.txt', 'a (2).txt']))).toBe('a (3).txt');
		expect(uniqueName('a', new Set(['a', 'a (2)']))).toBe('a (3)');
		expect(uniqueName('x.tar.gz', new Set(['x.tar.gz']))).toBe('x.tar (2).gz');
	});
});
