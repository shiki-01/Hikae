import { describe, it, expect } from 'vitest';
import { fileKindOf } from './file-kind';

describe('fileKindOf', () => {
	it('拡張子から種類を決める（大文字小文字を区別しない）', () => {
		expect(fileKindOf('第3章.docx')).toBe('document');
		expect(fileKindOf('資料/集計.XLSX')).toBe('table');
		expect(fileKindOf('図4.png')).toBe('image');
		expect(fileKindOf('メモ.txt')).toBe('text');
		expect(fileKindOf('報告書.pdf')).toBe('pdf');
	});

	it('拡張子が無い・隠しファイル・未知の拡張子は不明', () => {
		expect(fileKindOf('Makefile')).toBe('unknown');
		expect(fileKindOf('.gitignore')).toBe('unknown');
		expect(fileKindOf('data.bin')).toBe('unknown');
		expect(fileKindOf('dir.v2/README')).toBe('unknown');
	});

	it('フォルダ名の中の点は拡張子として扱わない', () => {
		expect(fileKindOf('a.b/c.txt')).toBe('text');
		expect(fileKindOf('a\\b.docx\\noext')).toBe('unknown');
	});
});
