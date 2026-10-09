// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { goto } from '../../../test/app-navigation';
import { setPageUrl } from '../../../test/app-state';
import { t, type MessageKey } from '#lib/i18n/index.js';
import SettingsScreen from './SettingsScreen.svelte';

setupSmoke();

const TABS: MessageKey[] = [
	'settings.general',
	'settings.fetch',
	'settings.push',
	'settings.auto_save',
	'settings.ignore',
	'settings.ai',
	'settings.extensions',
	'settings.advanced'
];

beforeEach(() => {
	setPageUrl('/settings');
	goto.mockClear();
	localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: true, onboarded: true }));
	localStorage.removeItem('hikae.mock.reauth');
});

describe('SettingsScreen のスモークテスト', () => {
	it('例外なく描画され、すべてのタブを切り替えられる', async () => {
		renderWithClient(SettingsScreen);
		expect(screen.getByRole('heading', { name: t('settings.title') })).toBeTruthy();
		for (const key of TABS) {
			await fireEvent.click(screen.getByRole('tab', { name: t(key) }));
			expect(screen.getByRole('tab', { name: t(key), selected: true })).toBeTruthy();
			expect(await screen.findByRole('heading', { name: t(key) })).toBeTruthy();
		}
	});

	it('プロジェクトを指定して開くと、そのプロジェクト用の切り替えが出る', async () => {
		setPageUrl('/settings?project=proj1&tab=auto_save');
		renderWithClient(SettingsScreen);
		const links = await screen.findAllByText(t('settings.project_only'));
		await fireEvent.click(links[0]);
		expect(await screen.findByText(t('settings.scope_all'))).toBeTruthy();
		expect(screen.getByRole('tab', { name: t('settings.auto_save'), selected: true })).toBeTruthy();
	});

	it('一般のタブに GitHub アカウントが出て、ログアウトの確認から、ログインの手順へ移る', async () => {
		renderWithClient(SettingsScreen);
		expect(await screen.findByTestId('account-name')).toBeTruthy();
		expect(screen.getByTestId('account-name').textContent?.trim()).toBe('shiki-01');
		await fireEvent.click(screen.getByRole('button', { name: t('account.logout') }));
		expect(await screen.findByText(t('account.logout_title'))).toBeTruthy();
		expect(screen.getByText(t('account.logout_text'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('account.logout_confirm') }));
		await vi.waitFor(() =>
			expect(goto).toHaveBeenCalledWith('/welcome?return=%2F', { replaceState: true })
		);
		expect(JSON.parse(localStorage.getItem('hikae.session') ?? '{}')).toMatchObject({
			loggedIn: false,
			onboarded: true
		});
	});

	it('確認でキャンセルすると、ログアウトしない', async () => {
		renderWithClient(SettingsScreen);
		await fireEvent.click(await screen.findByRole('button', { name: t('account.logout') }));
		await fireEvent.click(
			await screen.findByRole('button', { name: t('settings.confirm_cancel') })
		);
		expect(goto).not.toHaveBeenCalled();
		expect(JSON.parse(localStorage.getItem('hikae.session') ?? '{}').loggedIn).toBe(true);
	});

	it('トークンが切れているときは、再ログインのボタンが出る', async () => {
		localStorage.setItem('hikae.mock.reauth', '1');
		renderWithClient(SettingsScreen);
		expect(await screen.findByText(t('account.reauth'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('account.relogin') }));
		expect(goto).toHaveBeenCalledWith(`/welcome?return=${encodeURIComponent('/settings')}`, {
			replaceState: true
		});
	});

	it('一般のタブに「元に戻した後に自動で保存」があり、切り替えると保存される', async () => {
		renderWithClient(SettingsScreen);
		await screen.findByRole('heading', { name: t('settings.general') });
		expect(screen.getByText(t('settings.auto_save_after_restore_text'))).toBeTruthy();
		const toggle = await screen.findByRole('switch', {
			name: t('settings.auto_save_after_restore')
		});
		expect(toggle.getAttribute('aria-checked')).toBe('true');
		await fireEvent.click(toggle);
		await waitFor(() =>
			expect(
				screen
					.getByRole('switch', { name: t('settings.auto_save_after_restore') })
					.getAttribute('aria-checked')
			).toBe('false')
		);
		// 次に開いたときにも反映されている（モックの設定は端末に保存される）
		expect(localStorage.getItem('hikae.settings')).toContain('"autoSaveAfterRestore":false');
		localStorage.removeItem('hikae.settings');
	});

	it('ログインしていないときは、ログインのボタンが出る', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		renderWithClient(SettingsScreen);
		expect(await screen.findByText(t('account.not_logged_in'))).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('account.logout') })).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('account.login') }));
		expect(goto).toHaveBeenCalledWith(`/welcome?return=${encodeURIComponent('/settings')}`, {
			replaceState: true
		});
	});

	it('ログアウト直後でも設定画面は開け、戻るは一覧へ向かう（履歴は使わない）', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		const back = vi.spyOn(history, 'back');
		renderWithClient(SettingsScreen);
		await screen.findByText(t('account.not_logged_in'));
		await fireEvent.click(screen.getByRole('button', { name: t('settings.back') }));
		expect(goto).toHaveBeenCalledWith('/', { replaceState: true });
		expect(back).not.toHaveBeenCalled();
		back.mockRestore();
	});

	it('プロジェクトから開いた設定の戻るは、そのプロジェクトの画面へ向かう', async () => {
		setPageUrl('/settings?project=proj1&tab=auto_save');
		renderWithClient(SettingsScreen);
		await fireEvent.click(screen.getByRole('button', { name: t('settings.back') }));
		expect(goto).toHaveBeenCalledWith('/project?id=proj1', { replaceState: true });
	});

	it('プロジェクトごとの設定に切り替えると、アカウントは出さない', async () => {
		setPageUrl('/settings?project=proj1');
		renderWithClient(SettingsScreen);
		await screen.findByTestId('account-name');
		const links = await screen.findAllByText(t('settings.project_only'));
		await fireEvent.click(links[0]);
		expect(await screen.findByText(t('settings.scope_all'))).toBeTruthy();
		expect(screen.queryByTestId('account-name')).toBeNull();
	});
});
