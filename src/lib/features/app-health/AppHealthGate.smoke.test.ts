// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import GateHost from '../../../test/GateHost.svelte';
import { t } from '#lib/i18n/index.js';
import GitUnavailableScreen from './GitUnavailableScreen.svelte';

setupSmoke();

afterEach(() => localStorage.removeItem('hikae.mock.parts'));

describe('AppHealthGate のスモークテスト（E17）', () => {
	it('保存に必要な部品が使えるときは、通常の画面を出す', async () => {
		renderWithClient(GateHost);
		expect(await screen.findByText('gate-child')).toBeTruthy();
		expect(screen.queryByText(t('app_health.title.not_found'))).toBeNull();
	});

	it('部品が見つからないときは、通常の画面を出さず、3 要素の案内を出す', async () => {
		localStorage.setItem('hikae.mock.parts', 'missing');
		renderWithClient(GateHost);
		expect(await screen.findByText(t('app_health.title.not_found'))).toBeTruthy();
		expect(screen.getByText(t('app_health.next'))).toBeTruthy();
		expect(screen.getByText(t('app_health.safe'))).toBeTruthy();
		expect(screen.queryByText('gate-child')).toBeNull();
		// Git の用語を画面に出さない
		expect(document.body.textContent ?? '').not.toMatch(/git/i);
	});

	it('もう一度確認すると、部品が戻っていれば通常の画面に進む', async () => {
		localStorage.setItem('hikae.mock.parts', 'missing');
		renderWithClient(GateHost);
		await screen.findByText(t('app_health.title.not_found'));
		localStorage.removeItem('hikae.mock.parts');
		await fireEvent.click(screen.getByRole('button', { name: t('app_health.retry') }));
		await waitFor(() => expect(screen.getByText('gate-child')).toBeTruthy());
	});
});

describe('GitUnavailableScreen のスモークテスト', () => {
	it('起動できない場合は専用の見出しを出し、入手元を詳細に示す', () => {
		renderWithClient(GitUnavailableScreen, {
			health: {
				partsAvailable: false,
				partsSource: 'path' as const,
				partsProblem: 'broken' as const
			},
			onretry: () => {}
		});
		expect(screen.getByText(t('app_health.title.broken'))).toBeTruthy();
		expect(
			screen.getByText(t('app_health.source', { source: t('app_health.source_path') }))
		).toBeTruthy();
	});
});
