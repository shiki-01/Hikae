// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { goto } from '../../../test/app-navigation';
import { t } from '#lib/i18n/index.js';
import ProjectScreen from './ProjectScreen.svelte';

setupSmoke();

describe('ProjectScreen のスモークテスト', () => {
	it('例外なく描画され、プロジェクト名と変更タブが出る', async () => {
		renderWithClient(ProjectScreen, { projectId: 'proj1' });
		expect(await screen.findByText(t('mock.project.thesis'))).toBeTruthy();
		expect(screen.getByRole('radio', { name: t('tabs.changes') })).toBeTruthy();
		expect(screen.getByRole('radio', { name: t('tabs.history') })).toBeTruthy();
		expect((await screen.findAllByText(t('mock.file.chapter3'))).length).toBeGreaterThan(0);
	});

	it('履歴タブと変更タブを切り替えても例外が出ない', async () => {
		renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await fireEvent.click(screen.getByRole('radio', { name: t('tabs.history') }));
		expect(screen.getByRole('radio', { name: t('tabs.history'), checked: true })).toBeTruthy();
		await fireEvent.click(screen.getByRole('radio', { name: t('tabs.changes') }));
		expect(screen.getByRole('radio', { name: t('tabs.changes'), checked: true })).toBeTruthy();
	});

	it('「すべてのファイル」に切り替えるとツリーが出て、「変更のみ」に戻せる', async () => {
		renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await fireEvent.click(screen.getByRole('radio', { name: t('changes.view_tree') }));
		expect(await screen.findByLabelText(t('changes.tree_label'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('radio', { name: t('changes.view_changes') }));
		expect((await screen.findAllByText(t('mock.file.chapter3'))).length).toBeGreaterThan(0);
	});

	it('設定のリンクが設定画面へ遷移する', async () => {
		renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await fireEvent.click(screen.getByRole('button', { name: t('projects.settings') }));
		expect(goto).toHaveBeenCalled();
	});
});

describe('ProjectScreen: ファイルの追加', () => {
	function fileInput(container: HTMLElement): HTMLInputElement {
		const input = container.querySelector<HTMLInputElement>('input[type="file"]');
		if (!input) throw new Error('file input is missing');
		return input;
	}

	async function drop(container: HTMLElement, name: string) {
		await fireEvent.change(fileInput(container), {
			target: { files: [new File(['x'], name)] }
		});
	}

	it('同名のファイルがあると確認が出て、両方残す（既定）を選ぶと別名で追加される', async () => {
		const { container } = renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await drop(container, t('mock.file.memo'));

		expect(await screen.findByText(t('add_conflict.title'))).toBeTruthy();
		expect(
			(screen.getByRole('radio', { name: t('add_conflict.keep_both') }) as HTMLInputElement).checked
		).toBe(true);
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.confirm') }));

		// 別名にしたことが、結果のダイアログで分かる
		expect(await screen.findByText(t('add_files.renamed_heading'))).toBeTruthy();
		expect(screen.getAllByText('メモ (2).txt').length).toBeGreaterThan(0);
	});

	it('置き換えを選ぶと、置き換えたことが結果に出る', async () => {
		const { container } = renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await drop(container, t('mock.file.chapter3'));

		expect(await screen.findByText(t('add_conflict.title'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('radio', { name: t('add_conflict.replace') }));
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.confirm') }));

		expect(await screen.findByText(t('add_files.replaced_heading'))).toBeTruthy();
		expect(screen.getByText(t('add_files.replaced_detail'))).toBeTruthy();
	});

	it('確認でキャンセルすると、何も追加されない', async () => {
		const { container } = renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		await drop(container, t('mock.file.memo'));
		await screen.findByText(t('add_conflict.title'));
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.cancel') }));
		expect(screen.queryByText(t('add_files.title'))).toBeNull();
	});

	it('ツリー表示ではフォルダの行がドロップ先の印を持ち、変更のみ表示では持たない', async () => {
		const { container } = renderWithClient(ProjectScreen, { projectId: 'proj1' });
		await screen.findByText(t('mock.project.thesis'));
		expect(container.querySelector('[data-drop-folder]')).toBeNull();
		await fireEvent.click(screen.getByRole('radio', { name: t('changes.view_tree') }));
		await screen.findByLabelText(t('changes.tree_label'));
		expect(screen.getByText(t('changes.tree_drop_hint'))).toBeTruthy();
		expect(container.querySelector('[data-drop-folder]')).not.toBeNull();
		await fireEvent.click(screen.getByRole('radio', { name: t('changes.view_changes') }));
		expect(container.querySelector('[data-drop-folder]')).toBeNull();
	});
});
