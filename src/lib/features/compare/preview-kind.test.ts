import { describe, it, expect } from 'vitest';
import type { FileDiff } from '#lib/api/types.js';
import { compareRouteOf, previewKindOf, type CompareRouteInput } from './preview-kind';

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

describe('compareRouteOf', () => {
	const text: FileDiff = { kind: 'text', rows: [] };
	const info: FileDiff = { kind: 'info', fileKind: 'unknown', sizeBytes: 10, modifiedAt: null };
	const base: CompareRouteInput = {
		kind: 'other',
		renderers: 'ready',
		hasCompareRenderer: true,
		text: { pending: false, diff: text }
	};

	it('pdf と pptx は compare を宣言しないため、拡張があっても並べて比べる', () => {
		expect(compareRouteOf({ ...base, kind: 'pdf' })).toBe('side');
		expect(compareRouteOf({ ...base, kind: 'pptx' })).toBe('side');
	});

	it('compare に使える表示が無いときは並べて比べる（一覧の取得の失敗も同じ）', () => {
		expect(compareRouteOf({ ...base, kind: 'image', hasCompareRenderer: false })).toBe('side');
		expect(compareRouteOf({ ...base, renderers: 'failed' })).toBe('side');
	});

	it('一覧の取得の途中は保留する', () => {
		expect(compareRouteOf({ ...base, kind: 'docx', renderers: 'pending' })).toBe('pending');
	});

	it('画像・docx・xlsx は、拡張で描く', () => {
		expect(
			compareRouteOf({ ...base, kind: 'image', text: { pending: true, diff: undefined } })
		).toBe('frame');
		expect(compareRouteOf({ ...base, kind: 'docx' })).toBe('frame');
		expect(compareRouteOf({ ...base, kind: 'xlsx' })).toBe('frame');
	});

	it('その他は、結果が無いうちは保留し、情報のみなら並べ、テキストなら拡張で描く', () => {
		expect(compareRouteOf({ ...base, text: { pending: true, diff: undefined } })).toBe('pending');
		expect(compareRouteOf({ ...base, text: { pending: false, diff: info } })).toBe('side');
		expect(compareRouteOf(base)).toBe('frame');
		expect(
			compareRouteOf({
				...base,
				text: { pending: false, diff: { kind: 'too_large', sizeBytes: 2_000_000 } }
			})
		).toBe('frame');
	});
});
