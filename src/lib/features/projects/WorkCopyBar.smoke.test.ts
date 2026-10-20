// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import WorkCopyBar from './WorkCopyBar.svelte';

setupSmoke();

// 作業コピーの機能はオフが既定。このテストでは設定をオンにしてから描画する
beforeEach(async () => {
	await api.updateSettings(null, { workCopyEnabled: true });
});

describe('WorkCopyBar のスモークテスト', () => {
	it('元のまま（いま）が現在の表示になり、メニューに「別の案を試す」が出る', async () => {
		renderWithClient(WorkCopyBar, {
			projectId: 'proj2',
			onconflict: vi.fn(),
			onshowchanges: vi.fn()
		});
		const trigger = await screen.findByRole('button', { name: t('wc.original') });
		await fireEvent.click(trigger);
		expect(screen.getByRole('menu', { name: t('wc.menu') })).toBeTruthy();
		expect(screen.getByRole('menuitem', { name: t('wc.new') })).toBeTruthy();
	});

	it('別の案を試すと、名前を入れて作られ、現在の表示が案の名前になる', async () => {
		renderWithClient(WorkCopyBar, {
			projectId: 'proj2',
			onconflict: vi.fn(),
			onshowchanges: vi.fn()
		});
		await fireEvent.click(await screen.findByRole('button', { name: t('wc.original') }));
		await fireEvent.click(screen.getByRole('menuitem', { name: t('wc.new') }));

		const submit = screen.getByRole('button', { name: t('wc.name_submit') });
		expect((submit as HTMLButtonElement).disabled).toBe(true);
		await fireEvent.input(screen.getByLabelText(t('wc.name_label')), {
			target: { value: '見積もり案' }
		});
		await fireEvent.click(submit);

		await waitFor(() => expect(screen.getByRole('button', { name: '見積もり案' })).toBeTruthy(), {
			timeout: 5000
		});
	});

	it('未保存の変更があると作られず、「保存してから切り替えます」の案内が出る', async () => {
		renderWithClient(WorkCopyBar, {
			projectId: 'proj1',
			onconflict: vi.fn(),
			onshowchanges: vi.fn()
		});
		await fireEvent.click(await screen.findByRole('button', { name: t('wc.original') }));
		await fireEvent.click(screen.getByRole('menuitem', { name: t('wc.new') }));
		await fireEvent.input(screen.getByLabelText(t('wc.name_label')), {
			target: { value: '案' }
		});
		await fireEvent.click(screen.getByRole('button', { name: t('wc.name_submit') }));

		expect(await screen.findByText(t('wc.unsaved_title'), {}, { timeout: 5000 })).toBeTruthy();
		expect(screen.getByRole('button', { name: t('wc.show_changes') })).toBeTruthy();
	});
});
