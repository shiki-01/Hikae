// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { api } from '#lib/api/index.js';
import { t } from '#lib/i18n/index.js';
import AddProjectDialog from './AddProjectDialog.svelte';

setupSmoke();

afterEach(() => vi.restoreAllMocks());

async function pickFolder(path: string) {
	vi.spyOn(api, 'pickFolder').mockResolvedValue(path);
	renderWithClient(AddProjectDialog, {
		open: true,
		initialMode: 'existing' as const,
		onclose: () => {}
	});
	await fireEvent.click(await screen.findByRole('button', { name: t('add_project.browse') }));
}

describe('AddProjectDialog: 同期フォルダの注意', () => {
	it('同期フォルダを選ぶと注意が出て、「このまま追加する」を選ぶまで追加できない', async () => {
		await pickFolder('C:\\Users\\taro\\OneDrive\\ゼミ');
		expect(await screen.findByText(t('cloud_sync.message'))).toBeTruthy();
		const add = screen.getByRole('button', { name: t('add_project.button') });
		expect((add as HTMLButtonElement).disabled).toBe(true);

		await fireEvent.click(screen.getByRole('button', { name: t('cloud_sync.keep') }));
		await waitFor(() =>
			expect(
				(screen.getByRole('button', { name: t('add_project.button') }) as HTMLButtonElement)
					.disabled
			).toBe(false)
		);
		expect(screen.getByText(t('cloud_sync.kept'))).toBeTruthy();
	});

	it('「別のフォルダを選ぶ」で選び直すと、注意と選択が消える', async () => {
		await pickFolder('C:\\Users\\taro\\OneDrive\\ゼミ');
		await screen.findByText(t('cloud_sync.message'));
		vi.spyOn(api, 'pickFolder').mockResolvedValue('C:\\Users\\taro\\Documents\\ゼミ');
		await fireEvent.click(screen.getByRole('button', { name: t('cloud_sync.change') }));
		await waitFor(() => expect(screen.queryByText(t('cloud_sync.message'))).toBeNull());
		expect(
			(screen.getByRole('button', { name: t('add_project.button') }) as HTMLButtonElement).disabled
		).toBe(false);
	});

	it('普通のフォルダでは注意を出さない', async () => {
		await pickFolder('C:\\Users\\taro\\Documents\\ゼミ');
		await waitFor(() =>
			expect(screen.getByDisplayValue('C:\\Users\\taro\\Documents\\ゼミ')).toBeTruthy()
		);
		expect(screen.queryByText(t('cloud_sync.message'))).toBeNull();
	});
});
