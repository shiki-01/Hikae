// @vitest-environment jsdom
// ブラウザ単体（モック API）での言語の切り替えのスモークテスト（設計書 2.3）
import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import LocaleSync from '#lib/features/settings/LocaleSync.svelte';
import { currentLocale, setLocale } from '#lib/i18n/index.js';
import LocaleProbe from '../../test/LocaleProbe.svelte';
import { renderWithClient, setupSmoke } from '../../test/smoke';

setupSmoke();

function reset() {
	setLocale('ja');
	localStorage.removeItem('hikae.settings');
}

beforeEach(reset);
afterEach(reset);

describe('言語の切り替え（ブラウザ単体）', () => {
	it('テンプレート内の日時の表示が、言語の切り替えに追随する', async () => {
		render(LocaleProbe, { props: { at: new Date(2026, 9, 4, 18, 2) } });
		expect(screen.getByTestId('datetime').textContent).toContain('18:02');
		setLocale('en');
		await waitFor(() => expect(screen.getByTestId('datetime').textContent).toContain('PM'));
		setLocale('ja');
		await waitFor(() => expect(screen.getByTestId('datetime').textContent).toContain('18:02'));
	});

	it('設定の言語を変えると、その場で反映され、<html lang> も合わせる', async () => {
		const { client } = renderWithClient(LocaleSync);
		await waitFor(() => expect(currentLocale()).toBe('ja'));

		await api.updateSettings(null, { language: 'en' });
		await client.invalidateQueries({ queryKey: keys.settings(null) });
		await waitFor(() => expect(currentLocale()).toBe('en'));
		expect(document.documentElement.lang).toBe('en');

		await api.updateSettings(null, { language: 'ja' });
		await client.invalidateQueries({ queryKey: keys.settings(null) });
		await waitFor(() => expect(currentLocale()).toBe('ja'));
		expect(document.documentElement.lang).toBe('ja');
	});
});
