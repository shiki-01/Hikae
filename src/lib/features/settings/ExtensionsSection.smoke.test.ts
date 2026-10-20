// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import ExtensionsSection from './ExtensionsSection.svelte';

setupSmoke();

beforeEach(() => {
	localStorage.clear();
});

describe('ExtensionsSection のスモークテスト', () => {
	it('標準の拡張を一覧に出し、標準の有効スイッチは押せない', async () => {
		renderWithClient(ExtensionsSection);
		expect(await screen.findByText(t('mock.extension.office'))).toBeTruthy();
		expect(screen.getByText(t('mock.extension.preview'))).toBeTruthy();
		expect(screen.getAllByText(t('extensions.badge_builtin')).length).toBe(2);
		const toggle = screen.getByRole('switch', {
			name: t('extensions.enable_label', { name: t('mock.extension.office') })
		});
		expect((toggle as HTMLButtonElement).disabled).toBe(true);
	});

	it('開発者モードがオフのときは、フォルダから追加のボタンを出さない', async () => {
		renderWithClient(ExtensionsSection);
		await screen.findByText(t('mock.extension.office'));
		expect(screen.queryByRole('button', { name: t('extensions.add_folder') })).toBeNull();
	});

	it('開発者モードをオンにする前に、確認を出す。取りやめたらオフのまま', async () => {
		renderWithClient(ExtensionsSection);
		await screen.findByText(t('mock.extension.office'));
		await fireEvent.click(screen.getByRole('switch', { name: t('extensions.dev_toggle') }));
		expect(await screen.findByText(t('extensions.dev_confirm_title'))).toBeTruthy();
		expect(screen.getByText(t('extensions.dev_confirm_text'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('settings.confirm_cancel') }));
		expect(screen.queryByText(t('extensions.dev_confirm_title'))).toBeNull();
		expect(
			screen.getByRole('switch', { name: t('extensions.dev_toggle') }).getAttribute('aria-checked')
		).toBe('false');
	});

	it('確認でオンにすると、フォルダから追加のボタンが出る', async () => {
		renderWithClient(ExtensionsSection);
		await screen.findByText(t('mock.extension.office'));
		await fireEvent.click(screen.getByRole('switch', { name: t('extensions.dev_toggle') }));
		await fireEvent.click(
			await screen.findByRole('button', { name: t('extensions.dev_confirm_apply') })
		);
		expect(await screen.findByRole('button', { name: t('extensions.add_folder') })).toBeTruthy();
	});
});
