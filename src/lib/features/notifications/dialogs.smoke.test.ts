// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { goto } from '../../../test/app-navigation';
import { AppError } from '#lib/api/errors.js';
import { mapError } from '#lib/api/mappers.js';
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

function backendError(code: string, params: [string, string][] = []) {
	return mapError({
		code,
		params,
		what_happened: 'RUST_WHAT',
		data_is_safe: 'RUST_SAFE',
		next_action: 'RUST_NEXT',
		technical_info: 'raw'
	});
}

beforeEach(() => goto.mockClear());

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

	it('ErrorDialog: 認証切れのエラーでは、ログインし直すボタンがログインの手順へ移す', async () => {
		reportError(backendError('not_logged_in'));
		renderWithClient(ErrorDialog);
		expect(await screen.findByText(t('errcode.not_logged_in.what'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('errbtn.login') }));
		expect(goto).toHaveBeenCalledWith('/welcome?return=%2F');
		closeError();
	});

	it('ErrorDialog: 一時的な失敗は、元の操作を渡したときだけ「もう一度」が出て再実行できる', async () => {
		const retry = vi.fn();
		reportError(backendError('file_in_use', [['file', '第3章.docx']]), { retry });
		renderWithClient(ErrorDialog);
		expect(
			await screen.findByText(t('errcode.file_in_use.what', { file: '第3章.docx' }))
		).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('errbtn.retry') }));
		expect(retry).toHaveBeenCalledTimes(1);
		closeError();
	});

	it('ErrorDialog: 元の操作を渡さないエラーは、閉じるボタンだけになる', async () => {
		reportError(backendError('git_failed'));
		renderWithClient(ErrorDialog);
		expect(await screen.findByText(t('errcode.git_failed.what'))).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('errbtn.retry') })).toBeNull();
		expect(screen.getAllByRole('button', { name: t('error.close') }).length).toBeGreaterThan(0);
		closeError();
	});

	it('ErrorDialog: 権限不足は、管理者向けの説明をコピーするボタンが出て、ダイアログは開いたまま', async () => {
		reportError(backendError('github_forbidden'));
		renderWithClient(ErrorDialog);
		expect(await screen.findByText(t('errcode.github_forbidden.what'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('errbtn.copy_admin') }));
		expect(screen.getByText(t('errcode.github_forbidden.what'))).toBeTruthy();
		closeError();
	});

	it('ErrorDialog: 保存先に関わるエラーは、誘導先を渡すと誘導のボタンが出る', async () => {
		const onprimary = vi.fn();
		reportError(backendError('remote_name_taken', [['name', 'a/b']]), { onprimary });
		renderWithClient(ErrorDialog);
		await fireEvent.click(await screen.findByRole('button', { name: t('errbtn.guide_remote') }));
		expect(onprimary).toHaveBeenCalledTimes(1);
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
