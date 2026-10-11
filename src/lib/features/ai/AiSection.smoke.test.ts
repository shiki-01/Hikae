// @vitest-environment jsdom
import { screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { renderWithClient, setupSmoke } from '../../../test/smoke';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import AiSection from './AiSection.svelte';

setupSmoke();

beforeEach(() => {
	localStorage.removeItem('hikae.settings');
	localStorage.removeItem('hikae.settings.projects');
});

describe('AI タブの自分で用意した GGUF の表示', () => {
	it('入手元を自分で用意したものにすると、拡張子違いの理由と注意書きが出る', async () => {
		await api.updateSettings(null, {
			aiRunner: 'builtin',
			aiModelSource: 'custom-gguf',
			aiCustomGgufPath: 'C:\\models\\note.txt'
		});
		renderWithClient(AiSection);
		expect(await screen.findByText(t('ai.custom_reason.wrong-extension'))).toBeTruthy();
		expect(screen.getByText(t('ai.custom_caution'))).toBeTruthy();
		expect(screen.getByText(t('ai.custom_active'))).toBeTruthy();
	});

	it('使えるファイルのパスなら、OK の表示を出す', async () => {
		await api.updateSettings(null, {
			aiRunner: 'builtin',
			aiModelSource: 'custom-gguf',
			aiCustomGgufPath: 'C:\\models\\mine.gguf'
		});
		renderWithClient(AiSection);
		expect(await screen.findByText(t('ai.custom_ok'))).toBeTruthy();
	});

	it('パスが無いときは、理由を出さずに入力欄を出す', async () => {
		await api.updateSettings(null, {
			aiRunner: 'builtin',
			aiModelSource: 'custom-gguf',
			aiCustomGgufPath: ''
		});
		renderWithClient(AiSection);
		expect(await screen.findByText(t('ai.custom_pick'))).toBeTruthy();
		expect(screen.queryByText(t('ai.custom_ok'))).toBeNull();
	});
});
