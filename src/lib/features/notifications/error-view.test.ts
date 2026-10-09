import { describe, it, expect } from 'vitest';
import { AppError, ERROR_CODES, isAutoRecovering } from '#lib/api/errors.js';
import { mapError } from '#lib/api/mappers.js';
import { t } from '#lib/i18n/index.js';
import { describeError, primaryKindOfBackendCode, resolveBackendMessage } from './error-view';

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
			'open_failed',
			'discard_not_untracked',
			'discard_not_a_file',
			'discard_file_too_large',
			'discard_not_backed_up',
			'trash_failed',
			'add_too_many_files',
			'add_folder_too_deep',
			'project_folder_too_broad',
			'git_unavailable',
			'disk_full',
			'index_locked',
			'index_lock_stale',
			'unsupported_file_names'
		];
		const params = {
			count: 1,
			file: 'a.txt',
			name: 'x/y',
			operation: 'pull',
			limit: 1000,
			lock: '.git/index.lock'
		};
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

describe('バックエンドのコードによる次の行動ボタン', () => {
	const raw = {
		what_happened: 'RUST_WHAT',
		data_is_safe: 'RUST_SAFE',
		next_action: 'RUST_NEXT',
		technical_info: 'raw'
	};
	const view = (code: string, available?: { retry: boolean; guide: boolean }) =>
		describeError(mapError({ ...raw, code, params: [['file', 'a.txt']] }), available);

	it('認証が切れたときは、ログインし直すボタンになる', () => {
		const result = view('not_logged_in');
		expect(result.primaryKind).toBe('login');
		expect(result.primaryLabel).toBe(t('errbtn.login'));
		expect(result.autoRecovering).toBe(false);
	});

	it('権限が足りないときは、管理者向けの説明をコピーするボタンになる', () => {
		const result = view('github_forbidden');
		expect(result.primaryKind).toBe('copy');
		expect(result.primaryLabel).toBe(t('errbtn.copy_admin'));
		expect(result.copyText).toBe(t('error.E02.copy_text'));
	});

	it('一時的な失敗は「もう一度」になり、元の操作を再実行できないときは閉じるだけになる', () => {
		for (const code of ['file_in_use', 'git_failed', 'git_timeout', 'io_error']) {
			const result = view(code);
			expect(result.primaryKind, code).toBe('retry');
			expect(result.primaryLabel, code).toBe(t('errbtn.retry'));
			const noRetry = view(code, { retry: false, guide: true });
			expect(noRetry.primaryKind, code).toBe('close');
			expect(noRetry.primaryLabel, code).toBe(t('error.close'));
		}
	});

	it('別のアプリが操作中のとき（E14）は「もう一度」、容量不足・取り込めない名前・部品の欠落は閉じるだけ', () => {
		expect(view('index_locked').primaryKind).toBe('retry');
		expect(view('index_locked', { retry: false, guide: false }).primaryKind).toBe('close');
		// 古いロックは待っても消えないため、再実行を促さない（案内の通りに対応してもらう）
		for (const code of [
			'index_lock_stale',
			'disk_full',
			'unsupported_file_names',
			'git_unavailable'
		]) {
			const result = view(code);
			expect(result.primaryKind, code).toBe('close');
			expect(result.autoRecovering, code).toBe(false);
		}
	});

	it('取り込めない名前の文言に、件数と最初のファイル名が入る', () => {
		const result = describeError(
			mapError({
				...raw,
				code: 'unsupported_file_names',
				params: [
					['file', 'Aux.txt'],
					['count', '3']
				]
			})
		);
		expect(result.title).toContain('Aux.txt');
		expect(result.title).toContain('3');
	});

	it('保存先に関わるエラーは、行き先がある場合だけ誘導のボタンになる', () => {
		for (const code of ['remote_name_taken', 'remote_conflict']) {
			expect(view(code).primaryKind, code).toBe('guide');
			expect(view(code).primaryLabel, code).toBe(t('errbtn.guide_remote'));
			expect(view(code, { retry: true, guide: false }).primaryKind, code).toBe('close');
		}
	});

	it('対応表にないコードは閉じるだけ', () => {
		for (const code of ['invalid_input', 'database_error', 'conflict', 'something_new']) {
			const result = view(code);
			expect(result.primaryKind, code).toBe('close');
			expect(primaryKindOfBackendCode(code), code).toBe('close');
			expect(result.copyText).toBeUndefined();
		}
	});

	it('通信断と GitHub 側の障害は、ダイアログではなく自動回復のトースト扱いになる', () => {
		const offline = view('network_unavailable');
		expect(offline.autoRecovering).toBe(true);
		expect(offline.toastMessage).toBe(t('error.E03.toast'));
		expect(offline.primaryKind).toBe('close');

		const down = view('github_unavailable');
		expect(down.autoRecovering).toBe(true);
		expect(down.toastMessage).toBe(t('error.E04.toast'));
		expect(down.primaryKind).toBe('retry');
		expect(down.primaryLabel).toBe(t('error.E04.button'));
		expect(view('github_unavailable', { retry: false, guide: false }).primaryKind).toBe('close');

		for (const code of ['file_in_use', 'git_failed', 'not_logged_in']) {
			expect(view(code).autoRecovering, code).toBe(false);
			expect(view(code).toastMessage, code).toBeUndefined();
		}
	});

	it('自動回復の判定は、画面側のコードとバックエンドのコードの両方に効く', () => {
		expect(isAutoRecovering('E03')).toBe(true);
		expect(isAutoRecovering('E04')).toBe(true);
		expect(isAutoRecovering('network_unavailable')).toBe(true);
		expect(isAutoRecovering('github_unavailable')).toBe(true);
		expect(isAutoRecovering('E12')).toBe(false);
		expect(isAutoRecovering('git_failed')).toBe(false);
		expect(isAutoRecovering('toString')).toBe(false);
	});

	it('画面側の E01 はログインし直すボタン、E12 は再実行できないとき閉じるだけ', () => {
		expect(describeError(new AppError('E01', '')).primaryKind).toBe('login');
		const noRetry = describeError(new AppError('E12', '', { name: 'x' }), {
			retry: false,
			guide: false
		});
		expect(noRetry.primaryKind).toBe('close');
		expect(noRetry.primaryLabel).toBe(t('error.close'));
	});
});
