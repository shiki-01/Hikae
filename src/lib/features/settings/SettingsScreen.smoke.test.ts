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

	it('詳細設定の「作業コピーを使う」は、全体の設定を切り替える（プロジェクトの上書きは作らない）', async () => {
		setPageUrl('/settings?project=proj1');
		renderWithClient(SettingsScreen);
		await fireEvent.click(await screen.findByRole('tab', { name: t('settings.advanced') }));
		await fireEvent.click(await screen.findByText(t('settings.advanced_toggle')));
		const toggle = await screen.findByRole('switch', { name: t('settings.work_copy') });
		expect(toggle.getAttribute('aria-checked')).toBe('false');
		await fireEvent.click(toggle);
		await vi.waitFor(() =>
			expect(
				screen.getByRole('switch', { name: t('settings.work_copy') }).getAttribute('aria-checked')
			).toBe('true')
		);
		// 作業コピーはプロジェクト別に上書きできないため、「このプロジェクトだけ変更」は出さない
		expect(screen.queryByText(t('settings.project_only'))).toBeNull();
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
			expect(goto).toHaveBeenCalledWith('/welcome?return=%2F', { replace: true })
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
			replace: true
		});
	});

	it('一般のタブに「元に戻した後に自動で保存」があり、切り替えると保存される', async () => {
		renderWithClient(SettingsScreen);
		await screen.findByRole('heading', { name: t('settings.general') });
		expect(screen.getByText(t('settings.auto_save_after_restore_text'))).toBeTruthy();
		const toggle = await screen.findByRole('switch', {
			name: t('settings.auto_save_after_restore')
		});
		// 既定はオフ（戻しただけで勝手に保存しない）
		expect(toggle.getAttribute('aria-checked')).toBe('false');
		await fireEvent.click(toggle);
		await waitFor(() =>
			expect(
				screen
					.getByRole('switch', { name: t('settings.auto_save_after_restore') })
					.getAttribute('aria-checked')
			).toBe('true')
		);
		// 次に開いたときにも反映されている（モックの設定は端末に保存される）
		expect(localStorage.getItem('hikae.settings')).toContain('"autoSaveAfterRestore":true');
		localStorage.removeItem('hikae.settings');
	});

	it('ログインしていないときは、ログインのボタンが出る', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		renderWithClient(SettingsScreen);
		expect(await screen.findByText(t('account.not_logged_in'))).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('account.logout') })).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('account.login') }));
		expect(goto).toHaveBeenCalledWith(`/welcome?return=${encodeURIComponent('/settings')}`, {
			replace: true
		});
	});

	it('ログアウト直後でも設定画面は開け、戻るは一覧へ向かう（履歴は使わない）', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: true }));
		const back = vi.spyOn(history, 'back');
		renderWithClient(SettingsScreen);
		await screen.findByText(t('account.not_logged_in'));
		await fireEvent.click(screen.getByRole('button', { name: t('settings.back') }));
		expect(goto).toHaveBeenCalledWith('/', { replace: true });
		expect(back).not.toHaveBeenCalled();
		back.mockRestore();
	});

	it('プロジェクトから開いた設定の戻るは、そのプロジェクトの画面へ向かう', async () => {
		setPageUrl('/settings?project=proj1&tab=auto_save');
		renderWithClient(SettingsScreen);
		await fireEvent.click(screen.getByRole('button', { name: t('settings.back') }));
		expect(goto).toHaveBeenCalledWith('/project?id=proj1', { replace: true });
	});

	it('自動保存のクラウド控えは、オンにする前に確認ダイアログを出し、取りやめると変えない', async () => {
		setPageUrl('/settings?tab=auto_save');
		renderWithClient(SettingsScreen);
		const toggle = await screen.findByRole('switch', { name: t('settings.cloud_backup') });
		expect(toggle.getAttribute('aria-checked')).toBe('false');
		await fireEvent.click(toggle);
		expect(await screen.findByText(t('settings.confirm_cloud_backup_title'))).toBeTruthy();
		expect(screen.getByText(t('settings.confirm_cloud_backup_text'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('settings.confirm_cancel') }));
		await vi.waitFor(() =>
			expect(
				screen
					.getByRole('switch', { name: t('settings.cloud_backup') })
					.getAttribute('aria-checked')
			).toBe('false')
		);
	});

	it('プロジェクトから開くと、自動保存の件数（この PC のみ）を出す', async () => {
		setPageUrl('/settings?project=proj1&tab=auto_save');
		renderWithClient(SettingsScreen);
		// モックは控えをオフにしているため、この PC にだけある件数を出す
		expect(
			await screen.findByText(t('settings.cloud_status_local_only', { local: 3 }))
		).toBeTruthy();
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
