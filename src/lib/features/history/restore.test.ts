import { describe, it, expect } from 'vitest';
import { formatDateTime } from '#lib/i18n/format.js';
import { restoreSaveMemo, scopeKey } from './restore';

describe('元に戻した後の保存のメモ', () => {
	const when = new Date(2026, 9, 4, 18, 2);

	it('全体を戻すときは、戻した時点の日時を入れる', () => {
		const memo = restoreSaveMemo({ kind: 'all' }, when);
		expect(memo).toBe(`${formatDateTime(when)} の状態に戻しました`);
	});

	it('1 ファイルを戻すときは、ファイル名（フォルダを除く）と日時を入れる', () => {
		const memo = restoreSaveMemo({ kind: 'file', path: '資料/第3章.docx' }, when);
		expect(memo).toBe(`「第3章.docx」を ${formatDateTime(when)} の状態に戻しました`);
	});

	it('範囲の識別子は、全体とファイルで変わる', () => {
		expect(scopeKey({ kind: 'all' })).toBe('all');
		expect(scopeKey({ kind: 'file', path: 'a.txt' })).toBe('file:a.txt');
	});
});
