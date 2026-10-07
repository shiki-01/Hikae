import { describe, it, expect } from 'vitest';
import { AppError, ERROR_CODES } from '#lib/api/errors.js';
import { describeError } from './error-view';

describe('エラー表示の組み立て', () => {
	it('すべてのエラーコードに、何が起きたか・データの安否・ボタンの文言がある', () => {
		for (const code of ERROR_CODES) {
			const view = describeError(new AppError(code, 'raw', { name: 'x', count: 1, size: 1 }));
			expect(view.title.length, `${code} title`).toBeGreaterThan(0);
			expect(view.message.length, `${code} message`).toBeGreaterThan(0);
			expect(view.primaryLabel.length, `${code} button`).toBeGreaterThan(0);
			expect(view.title, `${code} title`).not.toMatch(/\{\w+\}/);
		}
	});

	it('E03 と E04 は自動で回復するのでトースト扱いになる', () => {
		expect(describeError(new AppError('E03', '')).autoRecovering).toBe(true);
		expect(describeError(new AppError('E04', '')).autoRecovering).toBe(true);
		expect(describeError(new AppError('E12', '')).autoRecovering).toBe(false);
	});

	it('ファイル名などの値が文言に差し込まれる', () => {
		const view = describeError(new AppError('E12', 'EBUSY', { name: 'report.docx' }));
		expect(view.title).toContain('report.docx');
		expect(view.primaryKind).toBe('retry');
		expect(view.technical).toBe('EBUSY');
	});

	it('選択を求めるエラーには副ボタンがある', () => {
		expect(describeError(new AppError('E05', '')).secondaryLabel).toBeTruthy();
		expect(describeError(new AppError('E11', '', { name: 'x' })).secondaryLabel).toBeTruthy();
		expect(describeError(new AppError('E03', '')).secondaryLabel).toBeUndefined();
	});

	it('想定外のエラーも汎用の文言と技術情報で表示できる', () => {
		const view = describeError(new Error('boom'));
		expect(view.code).toBe('generic');
		expect(view.technical).toBe('boom');
		expect(view.primaryKind).toBe('close');
		expect(describeError('text').technical).toBe('text');
	});
});
