// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
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
