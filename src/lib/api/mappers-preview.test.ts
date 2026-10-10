import { describe, it, expect } from 'vitest';
import { mapFilePreview } from './mappers';
import type { FilePreview as BackendFilePreview } from '#lib/bindings.js';

describe('ファイルの内容の表示（プレビュー）の変換', () => {
	it('テキスト・段落・スライドは、中身をそのまま保つ', () => {
		expect(mapFilePreview({ kind: 'text', content: 'a\nb', truncated: false })).toEqual({
			kind: 'text',
			content: 'a\nb',
			truncated: false
		});
		expect(mapFilePreview({ kind: 'docx', paragraphs: ['x'], truncated: true })).toEqual({
			kind: 'docx',
			paragraphs: ['x'],
			truncated: true
		});
		expect(
			mapFilePreview({
				kind: 'pptx',
				slides: [{ index: 1, paragraphs: ['s'] }],
				truncated: false
			})
		).toEqual({ kind: 'pptx', slides: [{ index: 1, paragraphs: ['s'] }], truncated: false });
	});

	it('xlsx のシートは、行・列・セル・打ち切りを画面の形にする', () => {
		const result: BackendFilePreview = {
			kind: 'xlsx',
			sheets: [
				{
					name: 'Sheet1',
					rows: 3,
					cols: 2,
					cells: [{ row: 0, col: 1, value: '120' }],
					truncated: true
				}
			]
		};
		expect(mapFilePreview(result)).toEqual({
			kind: 'xlsx',
			sheets: [
				{
					name: 'Sheet1',
					rows: 3,
					cols: 2,
					cells: [{ row: 0, col: 1, value: '120' }],
					truncated: true
				}
			]
		});
	});

	it('理由とファイルなしを、画面の表記にする', () => {
		expect(mapFilePreview({ kind: 'unavailable', reason: 'too-large' })).toEqual({
			kind: 'unavailable',
			reason: 'too_large'
		});
		expect(mapFilePreview({ kind: 'unavailable', reason: 'unsupported-format' })).toEqual({
			kind: 'unavailable',
			reason: 'unsupported_format'
		});
		expect(mapFilePreview({ kind: 'unavailable', reason: 'corrupt' })).toEqual({
			kind: 'unavailable',
			reason: 'corrupt'
		});
		expect(mapFilePreview({ kind: 'unavailable', reason: 'binary' })).toEqual({
			kind: 'unavailable',
			reason: 'binary'
		});
		expect(mapFilePreview({ kind: 'not-found' })).toEqual({ kind: 'not_found' });
	});
});
