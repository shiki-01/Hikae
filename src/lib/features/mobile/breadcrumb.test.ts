import { describe, expect, it } from 'vitest';
import { breadcrumbsOf, joinRepoPath, parentPathOf } from './breadcrumb';

describe('パンくずとパスの扱い', () => {
	it('ルートにはパンくずが無い', () => {
		expect(breadcrumbsOf('')).toEqual([]);
	});

	it('階層ごとに、その階層までのパスを持つ', () => {
		expect(breadcrumbsOf('a/b/c')).toEqual([
			{ label: 'a', path: 'a' },
			{ label: 'b', path: 'a/b' },
			{ label: 'c', path: 'a/b/c' }
		]);
	});

	it('親のパスを返す（ルートの親はルート）', () => {
		expect(parentPathOf('a/b')).toBe('a');
		expect(parentPathOf('a')).toBe('');
		expect(parentPathOf('')).toBe('');
	});

	it('フォルダと名前をつなぐ（ルートは名前だけ）', () => {
		expect(joinRepoPath('', 'x.txt')).toBe('x.txt');
		expect(joinRepoPath('a/b', 'x.txt')).toBe('a/b/x.txt');
	});
});
