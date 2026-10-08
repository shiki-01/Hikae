// @vitest-environment jsdom
import { screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { AppError } from '#lib/api/errors.js';
import { t } from '#lib/i18n/index.js';
import AddProjectDialog from '../projects/AddProjectDialog.svelte';
import ConnectRemoteDialog from '../projects/ConnectRemoteDialog.svelte';
import InterruptedDialog from '../projects/InterruptedDialog.svelte';
import PullSaveDialog from '../projects/PullSaveDialog.svelte';
import SizeCheckDialog from '../changes/SizeCheckDialog.svelte';
import ConflictModal from '../conflict/ConflictModal.svelte';
import ErrorDialog from './ErrorDialog.svelte';
import { closeError, reportError } from './store.svelte';

setupSmoke();

const noop = () => {};

describe('ダイアログのスモークテスト', () => {
	it('SizeCheckDialog: 大きいファイルの一覧が出る', async () => {
		renderWithClient(SizeCheckDialog, {
			check: {
				blocked: [{ path: 'video\\huge.mov', sizeBytes: 200 * 1024 * 1024 }],
				warned: [{ path: 'photo.psd', sizeBytes: 60 * 1024 * 1024 }]
			},
			pending: false,
			onchoose: noop,
			oncancel: noop
		});
		expect(await screen.findByText(t('size_check.title'))).toBeTruthy();
		expect(screen.getByText('video/huge.mov')).toBeTruthy();
		expect(screen.getByText('photo.psd')).toBeTruthy();
	});

	it('InterruptedDialog: 途中で止まった操作の案内が出る', async () => {
		renderWithClient(InterruptedDialog, {
			open: true,
			operation: 'pull',
			onrecover: noop,
			onclose: noop
		});
		expect(await screen.findByText(t('interrupted.title'))).toBeTruthy();
		expect(screen.getByText(t('interrupted.recover'))).toBeTruthy();
	});

	it('PullSaveDialog: 未保存の件数つきの確認が出る', async () => {
		renderWithClient(PullSaveDialog, { unsavedCount: 3, onconfirm: noop, oncancel: noop });
		expect(await screen.findByText(t('pull_confirm.title'))).toBeTruthy();
		expect(screen.getByText(t('pull_confirm.what', { count: 3 }))).toBeTruthy();
	});

	it('ErrorDialog: 通知されたエラーの文言が出る', async () => {
		reportError(new AppError('E11', 'ENOENT: C:\\gone', { name: 'gone' }));
		renderWithClient(ErrorDialog);
		expect(await screen.findByText(t('error.E11.title', { name: 'gone' }))).toBeTruthy();
		expect(screen.getByText(t('error.E11.message'))).toBeTruthy();
		closeError();
	});

	it('ConflictModal: ぶつかったファイルの選択肢が出る', async () => {
		renderWithClient(ConflictModal, { open: true, projectId: 'proj1', onclose: noop });
		expect(await screen.findByText(t('conflict.title'))).toBeTruthy();
		expect((await screen.findAllByText(t('conflict.use_cloud'))).length).toBeGreaterThan(0);
	});

	it('AddProjectDialog: 追加する方法の選択が出る', async () => {
		renderWithClient(AddProjectDialog, { open: true, onclose: noop });
		expect(await screen.findByText(t('add_project.title'))).toBeTruthy();
		expect(screen.getByText(t('add_project.existing'))).toBeTruthy();
	});

	it('ConnectRemoteDialog: 接続の入力欄が出る', async () => {
		renderWithClient(ConnectRemoteDialog, {
			open: true,
			projectId: 'proj1',
			projectName: 'sample',
			onclose: noop
		});
		expect(await screen.findByText(t('connect.title', { name: 'sample' }))).toBeTruthy();
		expect(screen.getByText(t('connect.name'))).toBeTruthy();
	});
});
