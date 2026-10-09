// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { goto } from '../../../test/app-navigation';
import { t } from '#lib/i18n/index.js';
import ProjectsScreen from './ProjectsScreen.svelte';

setupSmoke();

beforeEach(() => {
	// 利用開始の手順を済ませた状態にして、ようこそ画面への遷移を起こさない
	localStorage.setItem('hikae.session', JSON.stringify({ loggedIn: true, onboarded: true }));
	goto.mockClear();
});

describe('ProjectsScreen のスモークテスト', () => {
	it('例外なく描画され、プロジェクトの一覧が出る', async () => {
		renderWithClient(ProjectsScreen);
		expect(screen.getByRole('heading', { name: t('projects.title') })).toBeTruthy();
		expect(await screen.findByText(t('mock.project.thesis'))).toBeTruthy();
		expect(screen.getByText(t('mock.project.materials'))).toBeTruthy();
		expect(goto).not.toHaveBeenCalled();
	});

	it('追加ボタンで、プロジェクトを追加するダイアログが開く', async () => {
		renderWithClient(ProjectsScreen);
		await screen.findByText(t('mock.project.thesis'));
		await fireEvent.click(screen.getByRole('button', { name: t('projects.add') }));
		expect(await screen.findByText(t('add_project.title'))).toBeTruthy();
	});

	it('プロジェクトを選ぶと、プロジェクト画面へ遷移する', async () => {
		renderWithClient(ProjectsScreen);
		await fireEvent.click(await screen.findByText(t('mock.project.thesis')));
		expect(goto).toHaveBeenCalledWith(expect.stringContaining('/project?id='));
	});
});

describe('ProjectsScreen: 一覧から外す', () => {
	async function openRemoveDialog(name: string) {
		await screen.findByText(name);
		await fireEvent.click(screen.getByRole('button', { name: t('project_card.menu', { name }) }));
		await fireEvent.click(await screen.findByRole('menuitem', { name: t('project_card.remove') }));
		await screen.findByText(t('remove_project.title', { name }));
	}

	it('カードのメニューから確認を経て、一覧から外せる', async () => {
		renderWithClient(ProjectsScreen);
		const name = t('mock.project.materials');
		await openRemoveDialog(name);

		// 確認: フォルダの中身と履歴は削除されないこと、自動保存とアップロードが止まることを伝える
		expect(screen.getByText(t('remove_project.safe'))).toBeTruthy();
		expect(screen.getByText(t('remove_project.effect'))).toBeTruthy();
		expect(screen.getByText(t('remove_project.again'))).toBeTruthy();

		await fireEvent.click(screen.getByRole('button', { name: t('remove_project.confirm') }));
		await waitFor(() => expect(screen.queryByText(name)).toBeNull());
	});

	it('確認でキャンセルすると、一覧に残る', async () => {
		renderWithClient(ProjectsScreen);
		const name = t('mock.project.thesis');
		await openRemoveDialog(name);
		await fireEvent.click(screen.getByRole('button', { name: t('remove_project.cancel') }));
		expect(screen.getByText(name)).toBeTruthy();
	});
});
