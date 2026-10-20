import { describe, it, expect, afterEach } from 'vitest';
import { t, setLocale, type MessageKey } from './index';
import ja from './ja';
import en from './en';
import { formatBytes, formatDateTime, formatRelative } from './format';

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
			'names',
			'when',
			'n',
			'max',
			'pc',
			'size',
			'title',
			'message',
			'current',
			'total',
			'path',
			'min',
			'unit',
			'file',
			'folder',
			'limit',
			'replaced',
			'source',
			'lock',
			'left',
			'right',
			'rate',
			'ref',
			'before',
			'after',
			'files',
			'value',
			'version',
			'formats',
			// AI（ローカル LLM）の表示
			'memory',
			'disk',
			'needed',
			'have',
			'received',
			'stage',
			'level',
			// 保存メモの詳細の目安（文字数と行数）
			'chars',
			'lines',
			'max_chars',
			'max_lines',
			// 自動保存のクラウド控え（この PC の件数と、クラウドの件数）
			'local',
			'cloud'
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

describe('英語の辞書（en.ts）', () => {
	afterEach(() => setLocale('ja'));

	it('ja.ts と同じキー集合を持つ', () => {
		expect(Object.keys(en).sort()).toEqual(Object.keys(ja).sort());
	});

	it('置き換え名が、同じキーで ja と en のあいだで一致する', () => {
		const names = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
		for (const key of Object.keys(ja) as MessageKey[]) {
			expect(names(en[key]), key).toEqual(names(ja[key]));
		}
	});

	it('英語の文言に Git 用語を含めない（mock. を除く）', () => {
		const banned = /\b(branch|commit|merge|checkout|push|pull|rebase|staging|repository)\b/i;
		const offenders = Object.entries(en)
			.filter(([key, value]) => !key.startsWith('mock.') && banned.test(value))
			.map(([key]) => key);
		expect(offenders).toEqual([]);
	});

	it('言語を切り替えると、t() は選んだ言語の辞書を引く', () => {
		for (const key of Object.keys(ja) as MessageKey[]) {
			setLocale('en');
			expect(t(key), key).toBe(en[key]);
			setLocale('ja');
			expect(t(key), key).toBe(ja[key]);
		}
	});

	it('言語を切り替えると、日時の表示も言語に合わせて変わる', () => {
		const date = new Date(2026, 9, 4, 18, 2);
		setLocale('ja');
		expect(formatDateTime(date)).toContain('18:02');
		setLocale('en');
		expect(formatDateTime(date)).toContain('PM');
	});
});
