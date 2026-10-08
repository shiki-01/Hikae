import { describe, it, expect } from 'vitest';
import { joinPath } from './path';

describe('パスの連結', () => {
	it('Windows 形式の親フォルダには \\ でつなぐ', () => {
		expect(joinPath('C:\\Users\\a\\Documents', '研究データ')).toBe(
			'C:\\Users\\a\\Documents\\研究データ'
		);
	});

	it('/ 区切りの親フォルダには / でつなぐ', () => {
		expect(joinPath('/Users/a/Documents', 'notes')).toBe('/Users/a/Documents/notes');
	});

	it('親フォルダ末尾の区切りは重ねない', () => {
		expect(joinPath('C:\\Users\\a\\', 'x')).toBe('C:\\Users\\a\\x');
		expect(joinPath('/Users/a/', 'x')).toBe('/Users/a/x');
	});
});
