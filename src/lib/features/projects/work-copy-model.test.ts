import { describe, expect, it } from 'vitest';
import type { WorkCopy } from '#lib/api/types.js';
import {
	archivable,
	canAdopt,
	DISPLAY_NAME_MAX_CHARS,
	isInvalidDisplayName,
	normalizeDisplayName
} from './work-copy-model';

function copy(partial: Partial<WorkCopy> & { branch: string }): WorkCopy {
	return {
		displayName: null,
		isCurrent: false,
		isOriginal: false,
		archived: false,
		createdAt: null,
		...partial
	};
}

describe('表示名の検査（設計書 4.9。バックエンドと同じ規則）', () => {
	it('前後の空白を除き、1〜40 文字を受け付ける', () => {
		expect(normalizeDisplayName('  見積もり案  ')).toBe('見積もり案');
		expect(normalizeDisplayName('あ'.repeat(DISPLAY_NAME_MAX_CHARS))).toBe('あ'.repeat(40));
	});

	it('空・空白だけ・41 文字以上は受け付けない', () => {
		expect(normalizeDisplayName('')).toBeNull();
		expect(normalizeDisplayName('   ')).toBeNull();
		expect(normalizeDisplayName('あ'.repeat(41))).toBeNull();
	});

	it('制御文字（改行・タブ）を含むものは受け付けない', () => {
		expect(normalizeDisplayName('案\nB')).toBeNull();
		expect(normalizeDisplayName('案\tB')).toBeNull();
	});

	it('入力中の注記は、空のときは出さず、送れない形のときだけ出す', () => {
		expect(isInvalidDisplayName('')).toBe(false);
		expect(isInvalidDisplayName('   ')).toBe(false);
		expect(isInvalidDisplayName('a'.repeat(41))).toBe(true);
		expect(isInvalidDisplayName('案A')).toBe(false);
	});
});

describe('メニューに出す操作', () => {
	const original = copy({ branch: 'main', isOriginal: true, isCurrent: false });
	const current = copy({ branch: 'wc-20261020-120000', isCurrent: true });
	const other = copy({ branch: 'wc-20261020-110000' });

	it('取り込みは、いまの作業コピーが元でないときだけ出す', () => {
		expect(canAdopt([original, current])).toBe(true);
		expect(canAdopt([{ ...original, isCurrent: true }])).toBe(false);
		expect(canAdopt([])).toBe(false);
	});

	it('しまうの対象は、いまの作業コピーと元のものを除く', () => {
		expect(archivable([original, current, other])).toEqual([other]);
	});
});
