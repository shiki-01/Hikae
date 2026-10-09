// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { goto } from '../../../test/app-navigation';
import { setPageUrl } from '../../../test/app-state';
import { t } from '#lib/i18n/index.js';
import WelcomeScreen from './WelcomeScreen.svelte';

setupSmoke();

beforeEach(() => {
	setPageUrl('/welcome');
	goto.mockClear();
});

describe('WelcomeScreen のスモークテスト', () => {
	it('例外なく描画され、最初の手順が出る', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: false }));
		renderWithClient(WelcomeScreen);
		expect(screen.getByRole('heading', { name: t('wizard.title') })).toBeTruthy();
		expect(await screen.findByRole('heading', { name: t('wizard.step1') })).toBeTruthy();
	});

	it('?return= があるときは、ログインし直すだけの画面になり、元の画面へ戻れる', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		setPageUrl(`/welcome?return=${encodeURIComponent('/project?id=proj1')}`);
		renderWithClient(WelcomeScreen);
		expect(screen.getByRole('heading', { name: t('relogin.title') })).toBeTruthy();
		expect(screen.queryByRole('heading', { name: t('wizard.title') })).toBeNull();
		expect(await screen.findByText(t('relogin.description'))).toBeTruthy();
		expect(await screen.findByRole('button', { name: t('wizard.login.button') })).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('wizard.next') })).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('relogin.back') }));
		expect(goto).toHaveBeenCalledWith('/project?id=proj1', { replaceState: true });
	});

	it('ログアウト直後（戻り先が設定画面）は、戻るボタンもログイン成功後も一覧へ向かう', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		setPageUrl(`/welcome?return=${encodeURIComponent('/settings')}`);
		renderWithClient(WelcomeScreen);
		await fireEvent.click(await screen.findByRole('button', { name: t('relogin.back') }));
		expect(goto).toHaveBeenLastCalledWith('/', { replaceState: true });
		goto.mockClear();
		await fireEvent.click(await screen.findByRole('button', { name: t('wizard.login.button') }));
		await vi.waitFor(() => expect(goto).toHaveBeenCalledWith('/', { replaceState: true }), {
			timeout: 5000
		});
	});

	it('?return= がログインの画面自身を指すときは無視して、通常の案内を出す', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: false }));
		setPageUrl(`/welcome?return=${encodeURIComponent('/welcome?return=%2F')}`);
		renderWithClient(WelcomeScreen);
		expect(screen.getByRole('heading', { name: t('wizard.title') })).toBeTruthy();
	});

	it('?return= がアプリ外を指すときは無視して、通常の案内を出す', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: false }));
		setPageUrl(`/welcome?return=${encodeURIComponent('https://example.com/')}`);
		renderWithClient(WelcomeScreen);
		expect(screen.getByRole('heading', { name: t('wizard.title') })).toBeTruthy();
		expect(await screen.findByRole('heading', { name: t('wizard.step1') })).toBeTruthy();
	});
});
