import { describe, it, expect } from 'vitest';
import { fileNameOf } from './discard';

describe('fileNameOf', () => {
	it('パスの最後の名前だけを返す', () => {
		expect(fileNameOf('図4.png')).toBe('図4.png');
		expect(fileNameOf('資料/新しい 版.txt')).toBe('新しい 版.txt');
		expect(fileNameOf('a\\b\\c.txt')).toBe('c.txt');
	});
});
