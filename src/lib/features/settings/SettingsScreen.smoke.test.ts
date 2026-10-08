// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
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

beforeEach(() => setPageUrl('/settings'));

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
});
