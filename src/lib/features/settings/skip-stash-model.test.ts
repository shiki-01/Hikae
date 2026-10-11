import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import type { SkipStashReason } from '#lib/api/types.js';
import { baseName, stashReasonKey, stashSavedDate } from './skip-stash-model';

describe('stashReasonKey', () => {
	const reasons: SkipStashReason[] = ['pending', 'deleted-upstream', 'unknown'];

	it('gives every reason a message that says what happened', () => {
		for (const reason of reasons) {
			expect(t(stashReasonKey(reason)).length).toBeGreaterThan(0);
		}
		expect(t(stashReasonKey('pending'))).toBe(
			'取り込み中に、この PC でも変更されていたため、書き戻していません'
		);
		expect(t(stashReasonKey('deleted-upstream'))).toBe(
			'クラウドで削除されたため、書き戻していません'
		);
	});

	it('uses no Git terms in the texts', () => {
		for (const reason of reasons) {
			expect(t(stashReasonKey(reason)).toLowerCase()).not.toMatch(/stash|index|skip|merge/);
		}
		expect(t('skip_stash.section_description').toLowerCase()).not.toMatch(/stash|index|skip|merge/);
	});
});

describe('baseName', () => {
	it('takes the last name with either separator', () => {
		expect(baseName('docs/a (この PC の退避 10-12).psd')).toBe('a (この PC の退避 10-12).psd');
		expect(baseName('dir\\sub\\b.txt')).toBe('b.txt');
		expect(baseName('top.txt')).toBe('top.txt');
	});
});

describe('stashSavedDate', () => {
	it('converts Unix seconds and keeps a missing time as null', () => {
		expect(stashSavedDate(null)).toBeNull();
		expect(stashSavedDate(0)?.getTime()).toBe(0);
		expect(stashSavedDate(1_800_000_000)?.getTime()).toBe(1_800_000_000_000);
	});
});
