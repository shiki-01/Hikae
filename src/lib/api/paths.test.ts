import { describe, it, expect } from 'vitest';
import { resolveInProject } from './paths';

describe('プロジェクト内パスの解決', () => {
	it('Windows のルートに相対パスを連結する', () => {
		expect(resolveInProject('C:\\work\\proj', 'docs/a.txt')).toBe('C:\\work\\proj\\docs\\a.txt');
		expect(resolveInProject('C:\\work\\proj\\', 'a.txt')).toBe('C:\\work\\proj\\a.txt');
	});

	it('POSIX のルートに相対パスを連結する', () => {
		expect(resolveInProject('/Users/me/proj', './docs//a.txt')).toBe('/Users/me/proj/docs/a.txt');
	});

	it('プロジェクト外を指すパスを拒否する', () => {
		expect(resolveInProject('/p', '../x')).toBeNull();
		expect(resolveInProject('/p', 'a/../../x')).toBeNull();
		expect(resolveInProject('C:\\p', 'a\\..\\x')).toBeNull();
		expect(resolveInProject('/p', '/etc/passwd')).toBeNull();
		expect(resolveInProject('C:\\p', 'D:\\x')).toBeNull();
		expect(resolveInProject('C:\\p', '\\\\server\\share')).toBeNull();
	});

	it('空の入力と NUL を拒否する', () => {
		expect(resolveInProject('/p', '')).toBeNull();
		expect(resolveInProject('/p', '.')).toBeNull();
		expect(resolveInProject('', 'a')).toBeNull();
		expect(resolveInProject('/p', 'a\0b')).toBeNull();
	});
});
