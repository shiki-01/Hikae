import { describe, expect, it } from 'vitest';
import { formatFileTypes } from './extension-model';

describe('formatFileTypes', () => {
	it('拡張子に . を付け、重複を除いて順番どおりに並べる', () => {
		expect(formatFileTypes(['docx', 'xlsx', 'docx'], '全部')).toBe('.docx、.xlsx');
	});

	it('全部の拡張子（*）は、指定の語に置き換えて末尾に付ける', () => {
		expect(formatFileTypes(['pdf', '*'], '全部')).toBe('.pdf、全部');
		expect(formatFileTypes(['*'], '全部')).toBe('全部');
	});

	it('形式が無いときは空文字を返す', () => {
		expect(formatFileTypes([], '全部')).toBe('');
	});
});
