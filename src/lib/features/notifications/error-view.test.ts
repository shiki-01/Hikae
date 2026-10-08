import { describe, it, expect } from 'vitest';
import { AppError, ERROR_CODES } from '#lib/api/errors.js';
import { mapError } from '#lib/api/mappers.js';
import { t } from '#lib/i18n/index.js';
import { describeError, resolveBackendMessage } from './error-view';

describe('エラー表示の組み立て', () => {
	it('すべてのエラーコードに、何が起きたか・データの安否・ボタンの文言がある', () => {
		for (const code of ERROR_CODES) {
			const view = describeError(new AppError(code, 'raw', { name: 'x', count: 1, size: 1 }));
			expect(view.title.length, `${code} title`).toBeGreaterThan(0);
			expect(view.message.length, `${code} message`).toBeGreaterThan(0);
			expect(view.primaryLabel.length, `${code} button`).toBeGreaterThan(0);
			expect(view.title, `${code} title`).not.toMatch(/\{\w+\}/);
		}
	});

	it('E03 と E04 は自動で回復するのでトースト扱いになる', () => {
		expect(describeError(new AppError('E03', '')).autoRecovering).toBe(true);
		expect(describeError(new AppError('E04', '')).autoRecovering).toBe(true);
		expect(describeError(new AppError('E12', '')).autoRecovering).toBe(false);
	});

	it('ファイル名などの値が文言に差し込まれる', () => {
		const view = describeError(new AppError('E12', 'EBUSY', { name: 'report.docx' }));
		expect(view.title).toContain('report.docx');
		expect(view.primaryKind).toBe('retry');
		expect(view.technical).toBe('EBUSY');
	});

	it('選択を求めるエラーには副ボタンがある', () => {
		expect(describeError(new AppError('E05', '')).secondaryLabel).toBeTruthy();
		expect(describeError(new AppError('E11', '', { name: 'x' })).secondaryLabel).toBeTruthy();
		expect(describeError(new AppError('E03', '')).secondaryLabel).toBeUndefined();
	});

	it('想定外のエラーも汎用の文言と技術情報で表示できる', () => {
		const view = describeError(new Error('boom'));
		expect(view.code).toBe('generic');
		expect(view.technical).toBe('boom');
		expect(view.primaryKind).toBe('close');
		expect(describeError('text').technical).toBe('text');
	});
});

describe('バックエンドのエラーコードによる文言', () => {
	const fallback = {
		what_happened: 'RUST_WHAT',
		data_is_safe: 'RUST_SAFE',
		next_action: 'RUST_NEXT',
		technical_info: 'raw'
	};

	it('文言のあるコードは ja.ts の 3 要素で表示する', () => {
		const view = describeError(mapError({ ...fallback, code: 'not_logged_in', params: [] }));
		expect(view.title).toBe(t('errcode.not_logged_in.what'));
		expect(view.message).toBe(
			`${t('errcode.not_logged_in.safe')}\n${t('errcode.not_logged_in.next')}`
		);
		expect(view.technical).toBe('raw');
	});

	it('params がプレースホルダに差し込まれる', () => {
		const view = describeError(
			mapError({ ...fallback, code: 'file_in_use', params: [['file', '第3章.docx']] })
		);
		expect(view.title).toContain('第3章.docx');
		expect(view.title).not.toContain('RUST_WHAT');
	});

	it('差し込む値が足りないときは Rust の文言を使う', () => {
		const view = describeError(mapError({ ...fallback, code: 'file_in_use', params: [] }));
		expect(view.title).toBe('RUST_WHAT');
		expect(view.message).toBe('RUST_SAFE\nRUST_NEXT');
	});

	it('未知のコードは Rust の文言をそのまま使う', () => {
		const view = describeError(mapError({ ...fallback, code: 'something_new', params: [] }));
		expect(view.title).toBe('RUST_WHAT');
		expect(view.message).toBe('RUST_SAFE\nRUST_NEXT');
	});

	it('画面に出うるバックエンドのエラーコードすべてに、3 要素の文言がある', () => {
		// app/src/lib.rs の AppError のコード一覧と、一覧に載らない復旧系のコード
		const codes = [
			'conflict',
			'git_failed',
			'git_timeout',
			'safety_check_failed',
			'io_error',
			'file_in_use',
			'invalid_input',
			'restore_point_not_found',
			'unexpected',
			'no_interrupted_operation',
			'not_recoverable',
			'file_not_found',
			'file_unreadable',
			'outside_project',
			'not_logged_in',
			'github_forbidden',
			'github_rate_limited',
			'network_unavailable',
			'github_unavailable',
			'login_not_configured',
			'keychain_error',
			'login_not_in_progress',
			'login_page_unexpected',
			'browser_open_failed',
			'github_error',
			'clone_invalid_repo',
			'clone_invalid_destination',
			'destination_not_empty',
			'destination_not_a_folder',
			'destination_unreadable',
			'remote_not_found',
			'clone_failed',
			'clone_timeout',
			'clone_register_failed',
			'remote_name_taken',
			'remote_name_invalid',
			'remote_owner_invalid',
			'remote_public_not_confirmed',
			'remote_already_connected',
			'project_folder_missing',
			'remote_conflict',
			'remote_not_writable',
			'remote_orphaned',
			'project_not_found',
			'project_already_registered',
			'folder_already_registered',
			'project_list_failed',
			'project_register_failed',
			'project_remove_failed',
			'relocate_not_a_project',
			'relocate_different_project',
			'relocate_cannot_verify',
			'relocate_failed',
			'database_error',
			'task_failed',
			'lock_failed',
			'settings_io_failed',
			'invalid_settings',
			'status_failed',
			'history_failed',
			'diff_failed',
			'change_list_failed',
			'conflict_list_failed',
			'preview_failed',
			'open_failed'
		];
		const params = { count: 1, file: 'a.txt', name: 'x/y', operation: 'pull' };
		for (const code of codes) {
			const resolved = resolveBackendMessage({
				code,
				params,
				whatHappened: 'RUST_WHAT',
				dataIsSafe: 'RUST_SAFE',
				nextAction: 'RUST_NEXT'
			});
			expect(resolved.whatHappened, code).not.toBe('RUST_WHAT');
			expect(resolved.dataIsSafe, code).not.toBe('RUST_SAFE');
			expect(resolved.nextAction, code).not.toBe('RUST_NEXT');
		}
	});

	it('復旧できないときの専用文言がある', () => {
		for (const code of ['restore_point_not_found', 'not_recoverable']) {
			const resolved = resolveBackendMessage({
				code,
				params: {},
				whatHappened: 'RUST_WHAT',
				dataIsSafe: 'RUST_SAFE',
				nextAction: 'RUST_NEXT'
			});
			expect(resolved.whatHappened, code).not.toBe('RUST_WHAT');
			expect(resolved.dataIsSafe, code).not.toBe('RUST_SAFE');
			expect(resolved.nextAction, code).not.toBe('RUST_NEXT');
		}
	});
});
