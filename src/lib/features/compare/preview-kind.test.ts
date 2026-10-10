import { describe, it, expect } from 'vitest';
import type { FileDiff } from '#lib/api/types.js';
import { diffBodyOf, previewKindOf } from './preview-kind';

describe('previewKindOf', () => {
	it('拡張子で種類を決める（大文字小文字は区別しない）', () => {
		expect(previewKindOf('写真/夏.PNG')).toBe('image');
		expect(previewKindOf('a.jpeg')).toBe('image');
		expect(previewKindOf('資料.PDF')).toBe('pdf');
		expect(previewKindOf('第3章.docx')).toBe('docx');
		expect(previewKindOf('dir\\Book.XLSX')).toBe('xlsx');
		expect(previewKindOf('発表.Pptx')).toBe('pptx');
	});

	it('画像・pdf・Office 以外は other にする（テキストかどうかは読み取り結果で決める）', () => {
		expect(previewKindOf('memo.txt')).toBe('other');
		expect(previewKindOf('logo.psd')).toBe('other');
		expect(previewKindOf('old.doc')).toBe('other');
		expect(previewKindOf('.pdf')).toBe('other');
		expect(previewKindOf('README')).toBe('other');
	});

	it('svg は画像として比べない（app-blob の対象外）', () => {
		expect(previewKindOf('icon.svg')).toBe('other');
	});
});

describe('diffBodyOf', () => {
	const text: FileDiff = { kind: 'text', rows: [] };
	const info: FileDiff = { kind: 'info', fileKind: 'unknown', sizeBytes: 10, modifiedAt: null };

	it('画像・docx・xlsx は、それぞれの比較で描く', () => {
		expect(diffBodyOf('image', undefined)).toBe('image');
		expect(diffBodyOf('docx', undefined)).toBe('docx');
		expect(diffBodyOf('xlsx', undefined)).toBe('xlsx');
	});

	it('pdf と pptx は、テキストの差分の結果に関わらず過去と今を並べる', () => {
		expect(diffBodyOf('pdf', undefined)).toBe('side_by_side');
		expect(diffBodyOf('pptx', text)).toBe('side_by_side');
	});

	it('その他は、結果が無いうちは保留し、情報のみなら並べ、テキストなら行差分にする', () => {
		expect(diffBodyOf('other', undefined)).toBe('pending');
		expect(diffBodyOf('other', info)).toBe('side_by_side');
		expect(diffBodyOf('other', text)).toBe('text');
		expect(diffBodyOf('other', { kind: 'too_large', sizeBytes: 2_000_000 })).toBe('text');
	});
});
