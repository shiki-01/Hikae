import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import type { FilePreview } from '#lib/api/types.js';
import {
	PDF_MAX_PAGES,
	isTruncatedPreview,
	pdfPageWindow,
	unavailableMessageKey
} from './preview-view';

describe('pdfPageWindow', () => {
	it('上限までは全ページを描き、超えたら上限で切って注記する', () => {
		expect(pdfPageWindow(12)).toEqual({ shown: 12, capped: false });
		expect(pdfPageWindow(PDF_MAX_PAGES)).toEqual({ shown: PDF_MAX_PAGES, capped: false });
		expect(pdfPageWindow(PDF_MAX_PAGES + 1)).toEqual({ shown: PDF_MAX_PAGES, capped: true });
	});
});

describe('isTruncatedPreview', () => {
	const sheet = (truncated: boolean) => ({
		name: 'Sheet1',
		rows: 1,
		cols: 1,
		cells: [],
		truncated
	});

	it('抽出の上限を超えた分を省いたものを、一部のみとみなす', () => {
		expect(isTruncatedPreview({ kind: 'text', content: '', truncated: true })).toBe(true);
		expect(isTruncatedPreview({ kind: 'docx', paragraphs: [], truncated: false })).toBe(false);
		expect(isTruncatedPreview({ kind: 'pptx', slides: [], truncated: true })).toBe(true);
		expect(isTruncatedPreview({ kind: 'xlsx', sheets: [sheet(false), sheet(true)] })).toBe(true);
		expect(isTruncatedPreview({ kind: 'xlsx', sheets: [sheet(false)] })).toBe(false);
	});

	it('内容が無い結果は、一部のみではない', () => {
		const none: FilePreview[] = [{ kind: 'not_found' }, { kind: 'unavailable', reason: 'binary' }];
		for (const preview of none) expect(isTruncatedPreview(preview)).toBe(false);
	});
});

describe('unavailableMessageKey', () => {
	it('理由ごとに、画面の文言のキーを返す', () => {
		expect(t(unavailableMessageKey('too_large'))).not.toBe(t(unavailableMessageKey('binary')));
		expect(unavailableMessageKey('corrupt')).toBe('preview.unavailable.corrupt');
	});
});
