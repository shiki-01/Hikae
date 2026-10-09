import { describe, expect, it } from 'vitest';
import {
	reloginSearch,
	resolveReloginTarget,
	resolveSettingsBackTarget,
	safeReturnPath
} from './relogin';

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

	it('ログインの画面そのものは戻り先にしない（往復を防ぐ）', () => {
		expect(safeReturnPath('/welcome')).toBeNull();
		expect(safeReturnPath('/welcome?return=%2F')).toBeNull();
		expect(safeReturnPath('/welcomeback')).toBe('/welcomeback');
	});

	it('ログイン後の行き先: 設定画面はプロジェクトか一覧、ログインの画面は一覧', () => {
		expect(resolveReloginTarget('/project?id=p1')).toBe('/project?id=p1');
		expect(resolveReloginTarget('/')).toBe('/');
		expect(resolveReloginTarget('/welcome')).toBe('/');
		expect(resolveReloginTarget('/settings')).toBe('/');
		expect(resolveReloginTarget('/settings?tab=push')).toBe('/');
		expect(resolveReloginTarget('/settings?project=p1&tab=push')).toBe('/project?id=p1');
	});

	it('設定の戻り先: プロジェクトを開いていればその画面、なければ一覧', () => {
		expect(resolveSettingsBackTarget('')).toBe('/');
		expect(resolveSettingsBackTarget('?tab=push')).toBe('/');
		expect(resolveSettingsBackTarget('?project=')).toBe('/');
		expect(resolveSettingsBackTarget('?project=p%201&tab=push')).toBe('/project?id=p%201');
	});
});
