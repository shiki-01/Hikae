import { describe, expect, it } from 'vitest';
import { base64ToBytes, bytesToBase64, fileToBase64, textToBase64 } from './base64';

describe('base64 の変換', () => {
	it('バイト列と相互に変換できる', () => {
		const bytes = new Uint8Array([0, 1, 2, 127, 128, 250, 255]);
		expect(base64ToBytes(bytesToBase64(bytes))).toEqual(bytes);
	});

	it('既知の値に一致する（UTF-8 のテキストを含む）', () => {
		expect(textToBase64('hello')).toBe('aGVsbG8=');
		// 「あ」の UTF-8 は E3 81 82
		expect(textToBase64('あ')).toBe('44GC');
	});

	it('大きなデータでも引数の上限を超えずに変換できる（16 MiB）', () => {
		const size = 16 * 1024 * 1024;
		const bytes = new Uint8Array(size);
		for (let index = 0; index < size; index += 1) bytes[index] = (index * 31) % 256;
		const encoded = bytesToBase64(bytes);
		expect(encoded.length).toBe(Math.ceil(size / 3) * 4);
		const decoded = base64ToBytes(encoded);
		expect(decoded.length).toBe(size);
		expect(decoded[size - 1]).toBe(bytes[size - 1]);
		expect(decoded[12345]).toBe(bytes[12345]);
	});

	it('File（Blob）の中身を base64 にできる', async () => {
		const file = new File([new Uint8Array([72, 105])], 'a.txt');
		expect(await fileToBase64(file)).toBe('SGk=');
	});
});
