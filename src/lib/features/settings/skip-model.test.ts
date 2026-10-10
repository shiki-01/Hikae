import { describe, it, expect } from 'vitest';
import type { SkipWorktreeEntry } from '#lib/api/types.js';
import { describeSkipNotice } from './skip-notice';
import {
	displayPath,
	NOTICE_NAME_LIMIT,
	skipEntryStatus,
	skipNoticeLines,
	summarizeNames
} from './skip-model';

describe('summarizeNames', () => {
	it('lists every name when there are few', () => {
		expect(summarizeNames(['a.psd', 'b.psd'])).toEqual({ names: 'a.psd、b.psd', more: 0 });
	});

	it('shows the first few names and counts the rest', () => {
		const paths = ['a', 'b', 'c', 'd', 'e'];
		expect(summarizeNames(paths)).toEqual({ names: 'a、b、c', more: 2 });
		expect(NOTICE_NAME_LIMIT).toBe(3);
	});

	it('normalizes Windows separators for display', () => {
		expect(summarizeNames(['dir\\a.psd']).names).toBe('dir/a.psd');
	});
});

describe('skipNoticeLines', () => {
	it('returns nothing when nothing needs telling', () => {
		expect(skipNoticeLines({ pending: [], removedUpstream: [] })).toEqual([]);
	});

	it('uses one line for pending copies and one for copies whose file the cloud removed', () => {
		const lines = skipNoticeLines({ pending: ['big.psd'], removedUpstream: ['old.psd'] });
		expect(lines.map((line) => line.key)).toEqual(['skip.notice_pending', 'skip.notice_removed']);
		expect(lines[0].values).toEqual({ names: 'big.psd', count: 1, more: 0 });
		expect(lines[1].values).toEqual({ names: 'old.psd', count: 1, more: 0 });
	});

	it('keeps the count of every name even when only some are shown', () => {
		const lines = skipNoticeLines({
			pending: ['a', 'b', 'c', 'd'],
			removedUpstream: []
		});
		expect(lines).toHaveLength(1);
		expect(lines[0].values).toEqual({ names: 'a、b、c', count: 4, more: 1 });
	});
});

describe('describeSkipNotice', () => {
	it('turns the notice into sentences that keep the three parts (what / safe / next)', () => {
		const sentences = describeSkipNotice({ pending: ['big.psd'], removedUpstream: [] });
		expect(sentences).toHaveLength(1);
		expect(sentences[0]).toContain('big.psd');
		expect(sentences[0]).toContain('退避に残っています');
		expect(sentences[0]).toContain('確認してください');
	});

	it('adds the "and N more" wording only when names were cut', () => {
		const [sentence] = describeSkipNotice({
			pending: ['a', 'b', 'c', 'd'],
			removedUpstream: []
		});
		expect(sentence).toContain('a、b、c ほか 1 件');
	});

	it('does not use Git terms in the text', () => {
		const sentences = describeSkipNotice({
			pending: ['big.psd'],
			removedUpstream: ['old.psd']
		});
		for (const sentence of sentences) {
			expect(sentence.toLowerCase()).not.toMatch(/stash|index|skip|update-index|merge/);
		}
	});
});

describe('skipEntryStatus and displayPath', () => {
	const entry = (ignored: boolean): SkipWorktreeEntry => ({
		path: 'a.psd',
		setAt: '2026-10-10T00:00:00Z',
		ignored,
		dismissed: false
	});

	it('marks an entry as ignored or as lost (the ignore was removed on this PC)', () => {
		expect(skipEntryStatus(entry(true))).toBe('ignored');
		expect(skipEntryStatus(entry(false))).toBe('lost');
	});

	it('shows paths with forward slashes', () => {
		expect(displayPath('dir\\sub\\a.psd')).toBe('dir/sub/a.psd');
	});
});
