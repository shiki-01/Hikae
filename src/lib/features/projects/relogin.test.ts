import { describe, expect, it } from 'vitest';
import { reloginSearch, safeReturnPath } from './relogin';

describe('再ログインの行き先', () => {
	it('いまの画面を戻り先にして、ログインの画面へ向かう', () => {
		expect(reloginSearch({ pathname: '/project', search: '?id=a%20b&x=1' })).toBe(
			`?return=${encodeURIComponent('/project?id=a%20b&x=1')}`
		);
		expect(reloginSearch({ pathname: '/', search: '' })).toBe('?return=%2F');
	});

	it('ログインの画面にいるときは、戻り先を付けない', () => {
		expect(reloginSearch({ pathname: '/welcome', search: '?return=%2F' })).toBe('');
	});

	it('戻り先はアプリ内の画面だけを受け付ける', () => {
		expect(safeReturnPath('/project?id=1')).toBe('/project?id=1');
		expect(safeReturnPath('/')).toBe('/');
		for (const bad of [
			null,
			'',
			'project',
			'https://example.com/',
			'//example.com/',
			'/\\example.com',
			'javascript:alert(1)',
			'/a\nb'
		]) {
			expect(safeReturnPath(bad), String(bad)).toBeNull();
		}
	});

	it('組み立てた値は、そのまま戻り先として受け付けられる', () => {
		const search = reloginSearch({ pathname: '/settings', search: '?tab=push&project=p1' });
		const param = new URL(`/welcome${search}`, 'http://localhost').searchParams.get('return');
		expect(safeReturnPath(param)).toBe('/settings?tab=push&project=p1');
	});
});
