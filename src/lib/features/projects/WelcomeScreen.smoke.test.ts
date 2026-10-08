// @vitest-environment jsdom
import { screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import WelcomeScreen from './WelcomeScreen.svelte';

setupSmoke();

describe('WelcomeScreen のスモークテスト', () => {
	it('例外なく描画され、最初の手順が出る', async () => {
		localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: false, onboarded: false }));
		renderWithClient(WelcomeScreen);
		expect(screen.getByRole('heading', { name: t('wizard.title') })).toBeTruthy();
		expect(await screen.findByRole('heading', { name: t('wizard.step1') })).toBeTruthy();
	});
});
