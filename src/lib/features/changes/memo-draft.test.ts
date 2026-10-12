import { describe, expect, it } from 'vitest';
import {
	applySuggestion,
	composeMemo,
	DETAIL_MAX_CHARS,
	DETAIL_MAX_LINES,
	detailMeter,
	memoBody,
	summaryLine
} from './memo-draft';

describe('composeMemo', () => {
	it('要約だけのときは要約を返す（前後の空白は除く）', () => {
		expect(composeMemo('  税率を変えた  ', '')).toBe('税率を変えた');
		expect(composeMemo('  税率を変えた  ', '   \n ')).toBe('税率を変えた');
	});

	it('詳細があるときは、要約の後に空行を挟んで詳細をつなぐ', () => {
		expect(composeMemo('要約', '・一行目\n・二行目\n')).toBe('要約\n\n・一行目\n・二行目');
	});

	it('要約が空なら、詳細だけを返す', () => {
		expect(composeMemo('', '詳細だけ')).toBe('詳細だけ');
	});
});

describe('detailMeter', () => {
	it('文字数と行数を数える。空なら 0', () => {
		expect(detailMeter('')).toEqual({ chars: 0, lines: 0, over: false });
		expect(detailMeter('あいう\nえ')).toEqual({ chars: 5, lines: 2, over: false });
	});

	it('目安の行数・文字数を超えると over になる', () => {
		const lines = Array.from({ length: DETAIL_MAX_LINES + 1 }, () => 'a').join('\n');
		expect(detailMeter(lines).over).toBe(true);
		expect(detailMeter('あ'.repeat(DETAIL_MAX_CHARS + 1)).over).toBe(true);
		expect(detailMeter('あ'.repeat(DETAIL_MAX_CHARS)).over).toBe(false);
	});
});

describe('applySuggestion', () => {
	it('要約は要約欄へ、本文は詳細欄へ入れ、直前の下書きを残す', () => {
		const current = { summary: '手書きの要約', detail: '手書きの詳細' };
		const { next, backup } = applySuggestion(current, { title: '提案の要約', body: '・一行目' });
		expect(next).toEqual({ summary: '提案の要約', detail: '・一行目' });
		expect(backup).toEqual(current);
		// 直前の下書きは別のオブジェクトとして残る（後から書き換えても影響しない）
		expect(backup).not.toBe(current);
	});

	it('本文が無いときは詳細を空にする', () => {
		const { next } = applySuggestion(
			{ summary: '', detail: '古い詳細' },
			{ title: '要約', body: null }
		);
		expect(next).toEqual({ summary: '要約', detail: '' });
	});
});

describe('summaryLine と memoBody（履歴の表示）', () => {
	it('1 行目の要約を取り出す（空の行は飛ばす）', () => {
		expect(summaryLine('\n  税率を変えた  \n\n・本文')).toBe('税率を変えた');
		expect(summaryLine('')).toBe('');
	});

	it('要約の後の本文を取り出す（前後の空行は除く）', () => {
		expect(memoBody('要約\n\n・一行目\n・二行目\n')).toBe('・一行目\n・二行目');
		expect(memoBody('要約だけ')).toBe('');
		expect(memoBody('要約\n\n')).toBe('');
		expect(memoBody('')).toBe('');
	});
});
