import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { autoSaveView, formatDelay } from './auto-save-status';

describe('自動保存の待ち時間の表示', () => {
	it('秒・分・時間で読みやすくする', () => {
		expect(formatDelay(10)).toBe('10 秒');
		expect(formatDelay(30)).toBe('30 秒');
		expect(formatDelay(60)).toBe('1 分');
		expect(formatDelay(120)).toBe('2 分');
		expect(formatDelay(600)).toBe('10 分');
		expect(formatDelay(90)).toBe('1 分 30 秒');
		expect(formatDelay(3600)).toBe('1 時間');
	});
});

describe('自動保存の説明', () => {
	const now = new Date('2026-10-08T12:00:00');

	it('オンのときは、いつ作られるかと、最後に作られた時刻を示す', () => {
		const view = autoSaveView(true, 120, new Date('2026-10-08T11:57:00'), now);
		expect(view).toEqual({
			kind: 'on',
			rule: 'ファイルを変更して 2 分操作しないと、自動で控えを残します。',
			last: '最後の自動保存: 3 分前'
		});
	});

	it('まだ一度も作られていないときは、その旨を示す', () => {
		const view = autoSaveView(true, 30, null, now);
		expect(view).toMatchObject({ kind: 'on', last: t('autosave.none') });
	});

	it('オフのときは、オフであることだけを示す', () => {
		expect(autoSaveView(false, 120, new Date('2026-10-08T11:57:00'), now)).toEqual({
			kind: 'off'
		});
	});
});
