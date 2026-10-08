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
