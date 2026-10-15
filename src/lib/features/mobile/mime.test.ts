import { describe, expect, it } from 'vitest';
import { extensionOf, extensionOfImageMime, mimeOf } from './mime';

describe('拡張子と MIME の型', () => {
	it('拡張子は小文字で返す。無い・先頭のドットだけのものは空', () => {
		expect(extensionOf('Photo.PNG')).toBe('png');
		expect(extensionOf('README')).toBe('');
		expect(extensionOf('.gitignore')).toBe('');
	});

	it('既知の拡張子から MIME の型を決める。分からなければ octet-stream', () => {
		expect(mimeOf('a.PNG')).toBe('image/png');
		expect(mimeOf('a.pdf')).toBe('application/pdf');
		expect(mimeOf('a.unknown')).toBe('application/octet-stream');
	});

	it('画像の MIME から拡張子を決める。対応しない型は null', () => {
		expect(extensionOfImageMime('image/jpeg')).toBe('jpg');
		expect(extensionOfImageMime('image/png')).toBe('png');
		expect(extensionOfImageMime('image/bmp')).toBeNull();
	});
});
