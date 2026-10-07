import { describe, it, expect } from 'vitest';
import { t } from './index';
import ja from './ja';
import { formatBytes, formatRelative } from './format';

describe('i18n', () => {
	it('定義済みキーの日本語文言を返す', () => {
		expect(t('app.name')).toBe('Hikae');
	});

	it('{名前} の位置に値を差し込む', () => {
		expect(t('header.status_unsaved', { count: 3 })).toContain('3');
		expect(t('restore.title', { time: '10/4 18:02' })).toBe('10/4 18:02 の状態に戻します');
	});

	it('画面に出す文言に Git 用語を含めない', () => {
		const banned =
			/\b(commit|push|pull|merge|conflict|clone|branch|repository|rebase|stash|checkout|HEAD|index)\b/i;
		const offenders = Object.entries(ja)
			.filter(([key, value]) => !key.startsWith('mock.') && banned.test(value))
			.map(([key]) => key);
		expect(offenders).toEqual([]);
	});

	it('置き換え位置の名前が、使う側と文言側で食い違わない', () => {
		const placeholders = /\{(\w+)\}/g;
		const known = new Set([
			'count',
			'time',
			'name',
			'when',
			'n',
			'max',
			'pc',
			'size',
			'title',
			'message',
			'current',
			'total'
		]);
		for (const [key, value] of Object.entries(ja)) {
			for (const match of value.matchAll(placeholders)) {
				expect(known.has(match[1]), `${key}: ${match[1]}`).toBe(true);
			}
		}
	});
});

describe('日時と大きさの表示', () => {
	const now = new Date('2026-10-07T12:00:00');

	it('相対時刻を分・時間・日で表す', () => {
		expect(formatRelative(new Date('2026-10-07T11:59:30'), now)).toBe(t('time.just_now'));
		expect(formatRelative(new Date('2026-10-07T11:30:00'), now)).toBe(
			t('time.minutes_ago', { n: 30 })
		);
		expect(formatRelative(new Date('2026-10-07T09:00:00'), now)).toBe(
			t('time.hours_ago', { n: 3 })
		);
		expect(formatRelative(new Date('2026-10-04T12:00:00'), now)).toBe(t('time.days_ago', { n: 3 }));
	});

	it('1週間以上前は月日で表す', () => {
		expect(formatRelative(new Date('2026-09-20T12:00:00'), now)).toBe('9/20');
	});

	it('バイト数を読みやすい単位にする', () => {
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(2048)).toBe('2.0 KB');
		expect(formatBytes(72 * 1024 * 1024)).toBe('72.0 MB');
	});
});
