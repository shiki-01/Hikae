import { describe, it, expect } from 'vitest';
import { describeError } from '#lib/features/notifications/error-view.js';
import {
	mapAddFilesResult,
	mapDiffResult,
	mapDeviceFlow,
	mapFileImpact,
	mapOverridden,
	mapOwner,
	mapPointChange,
	mapProjectTree,
	mapPull,
	mapProject,
	mapRemoteOutcome,
	mapRemoteProjects,
	mapSavePoint,
	mapSaveResult,
	mapSession,
	mapSettings,
	mapSettingsView,
	restoreFileFailure,
	toBackendPatch,
	toBackendSizeChoice
} from './mappers';

describe('ログインの変換', () => {
	it('ユーザー名とアバターの URL だけを取り出し、トークンに相当する値は持たない', () => {
		const session = mapSession({
			logged_in: true,
			reauth_required: false,
			onboarded: true,
			user: { login: 'shiki', avatar_url: 'https://example.com/a.png' }
		});
		expect(session).toEqual({
			loggedIn: true,
			onboarded: true,
			reauthRequired: false,
			userLogin: 'shiki',
			avatarUrl: 'https://example.com/a.png'
		});
	});

	it('https 以外・不正な URL のアバターは使わない', () => {
		const base = { logged_in: true, reauth_required: false, onboarded: true };
		for (const avatar of ['http://example.com/a.png', 'javascript:alert(1)', 'not a url']) {
			expect(
				mapSession({ ...base, user: { login: 'x', avatar_url: avatar } }).avatarUrl
			).toBeNull();
		}
		expect(mapSession({ ...base, user: { login: 'x', avatar_url: null } }).avatarUrl).toBeNull();
	});

	it('未ログインではユーザー名が無い', () => {
		const session = mapSession({
			logged_in: false,
			reauth_required: false,
			onboarded: false,
			user: null
		});
		expect(session.userLogin).toBeNull();
		expect(session.avatarUrl).toBeNull();
	});

	it('確認コード・確認ページ・有効期限を画面用にする', () => {
		expect(
			mapDeviceFlow({
				user_code: 'WDJB-MJHT',
				verification_uri: 'https://github.com/login/device',
				expires_in: 900,
				interval: 5
			})
		).toEqual({
			userCode: 'WDJB-MJHT',
			verificationUri: 'https://github.com/login/device',
			expiresInSecs: 900
		});
	});
});

describe('保存先と取得できるプロジェクトの変換', () => {
	it('保存先の作成可否を引き継ぐ', () => {
		expect(
			mapOwner({
				id: 'team',
				name: 'Team',
				kind: 'org',
				avatar_url: null,
				can_create: false,
				reason: 'members-cannot-create'
			})
		).toEqual({ id: 'team', name: 'Team', kind: 'org', canCreate: false });
	});

	it('取得できるプロジェクトと、一部しか取得していない印を変換する', () => {
		const list = mapRemoteProjects({
			projects: [
				{ id: 'me/a', name: 'a', owner_id: 'me', private: true, updated_at: null },
				{ id: 'team/b', name: 'b', owner_id: 'team', private: false, updated_at: '2026-01-01' }
			],
			truncated: true
		});
		expect(list.truncated).toBe(true);
		expect(list.projects).toEqual([
			{ id: 'me/a', name: 'a', ownerId: 'me', isPrivate: true },
			{ id: 'team/b', name: 'b', ownerId: 'team', isPrivate: false }
		]);
	});
});

describe('保存時点で変更されたファイルの変換', () => {
	it('名前変更のときだけ元のパスを持つ', () => {
		expect(mapPointChange({ path: 'new.txt', old_path: 'old.txt', kind: 'renamed' })).toEqual({
			path: 'new.txt',
			type: 'renamed',
			oldPath: 'old.txt'
		});
		const added = mapPointChange({ path: 'a.txt', old_path: null, kind: 'added' });
		expect(added).toEqual({ path: 'a.txt', type: 'added' });
		expect('oldPath' in added).toBe(false);
	});
});

describe('1 ファイルを戻す操作の変換', () => {
	it('上書きと作り直しだけを影響として表す', () => {
		const size = { size_now: 1, size_then: 2 };
		expect(mapFileImpact('a.txt', { kind: 'overwrite', ...size })).toEqual([
			{ path: 'a.txt', type: 'modified' }
		]);
		expect(mapFileImpact('a.txt', { kind: 'recreate', ...size })).toEqual([
			{ path: 'a.txt', type: 'restored' }
		]);
		expect(mapFileImpact('a.txt', { kind: 'unchanged', ...size })).toEqual([]);
		expect(mapFileImpact('a.txt', { kind: 'not-in-that-point', ...size })).toEqual([]);
	});

	it('戻せなかった理由を、データは無事であることと次の行動つきで表す', () => {
		for (const outcome of ['not-in-that-point', 'ignored-file-in-the-way'] as const) {
			const view = describeError(restoreFileFailure(outcome, 'a.txt'));
			expect(view.title).toContain('a.txt');
			expect(view.message).toContain('変更されていません');
			expect(view.message.split('\n')).toHaveLength(2);
		}
	});
});

describe('ファイル追加結果の変換', () => {
	it('区切りを / にし、理由を画面側の名前にする', () => {
		const outcome = mapAddFilesResult({
			added: [{ path: 'dir\\a (2).txt', renamed: true, large: false }],
			rejected: [
				{ name: 'big.mp4', reason: 'too-large', size: 200 },
				{ name: 'dir', reason: 'not-a-file', size: null },
				{ name: 'x', reason: 'unreadable', size: null }
			]
		});
		expect(outcome.added).toEqual([{ path: 'dir/a (2).txt', renamed: true, large: false }]);
		expect(outcome.rejected.map((f) => f.reason)).toEqual([
			'too_large',
			'not_a_file',
			'unreadable'
		]);
		expect(outcome.rejected[0].size).toBe(200);
	});
});

describe('設定の変換', () => {
	const backend = {
		onboarded: true,
		pull_on_startup: false,
		pull_interval_minutes: 5,
		save_before_pull: false,
		conflict_mode: 'show-dialog' as const,
		auto_push_after_save: true,
		push_reminder_hours: 1,
		auto_snapshot_enabled: false,
		auto_snapshot_delay_secs: 30,
		snapshot_retention_days: 365,
		memo_suggestion: 'rules' as const,
		auto_save_after_restore: false,
		large_file_warn_mb: 25,
		show_snapshots_in_timeline: 'shown' as const,
		open_action: 'default-app' as const,
		ai_runner: 'builtin' as const,
		ai_model: '',
		ai_scope: 'text-diff' as const,
		git_executable: 'bundled' as const,
		show_technical_info: false,
		default_visibility: 'private' as const,
		snapshot_size_cap_x: 2,
		ai_model_source: 'catalog' as const,
		ai_custom_gguf_path: '',
		work_copy_enabled: false,
		extension_dev_mode: false,
		extension_index_urls: [],
		git_lfs: false,
		term_display: 'plain' as const
	};

	it('画面で扱う項目だけを camelCase にする', () => {
		expect(mapSettings(backend)).toEqual({
			pullOnStartup: false,
			pullIntervalMinutes: 5,
			saveBeforePull: false,
			conflictMode: 'show-dialog',
			autoPushAfterSave: true,
			pushReminderHours: 1,
			autoSnapshotEnabled: false,
			autoSnapshotDelaySecs: 30,
			snapshotRetentionDays: 365,
			autoSaveAfterRestore: false,
			largeFileWarnMb: 25,
			showSnapshotsInTimeline: 'shown'
		});
	});

	it('上書きしている項目のキー名を画面側の名前に戻し、扱わない項目は除く', () => {
		expect(mapOverridden(['pull_interval_minutes', 'git_lfs', 'open_action'])).toEqual([
			'pullIntervalMinutes'
		]);
		const view = mapSettingsView({ settings: backend, overridden: ['large_file_warn_mb'] });
		expect(view.overridden).toEqual(['largeFileWarnMb']);
	});

	it('変更した項目だけをバックエンドの項目名で送る', () => {
		expect(toBackendPatch({ pullIntervalMinutes: 0, largeFileWarnMb: 25 })).toEqual({
			pull_interval_minutes: 0,
			large_file_warn_mb: 25
		});
		expect(toBackendPatch({})).toEqual({});
	});

	it('false や 0 も省略せず送る', () => {
		expect(toBackendPatch({ saveBeforePull: false })).toEqual({ save_before_pull: false });
	});
});

describe('取り込み結果の変換', () => {
	const base = { outcome: 'up-to-date', conflicts: [], size_check: null, unsaved_count: null };

	it('未保存の変更の確認が必要なときは、件数を持つ', () => {
		const result = mapPull({ ...base, outcome: 'needs-save-confirmation', unsaved_count: 3 });
		expect(result.unsavedCount).toBe(3);
		expect(result.mergedCount).toBe(0);
		expect(result.conflictCount).toBe(0);
		expect(result.sizeCheck).toBeNull();
	});

	it('そのほかの結果では確認の件数を持たない', () => {
		for (const outcome of ['up-to-date', 'fast-forwarded', 'merged', 'needs-size-decision']) {
			expect(mapPull({ ...base, outcome }).unsavedCount, outcome).toBeNull();
		}
		expect(mapPull({ ...base, outcome: 'merged' }).mergedCount).toBe(1);
	});
});

describe('保存結果の変換', () => {
	it('保存できたとき（大きいファイルの情報なし）は保存済みにする', () => {
		expect(mapSaveResult({ commit: 'abc', message: 'm', size_check: null })).toEqual({
			kind: 'saved'
		});
	});

	it('何も保存せず大きいファイルが返ったときは、確認が必要な結果にする（パスは変えない）', () => {
		const outcome = mapSaveResult({
			commit: null,
			message: null,
			size_check: {
				blocked: [{ path: '動画\\講義.mp4', size: 250 * 1024 * 1024 }],
				warned: [{ path: '素材.psd', size: null }]
			}
		});
		expect(outcome).toEqual({
			kind: 'size_check',
			check: {
				blocked: [{ path: '動画\\講義.mp4', sizeBytes: 250 * 1024 * 1024 }],
				warned: [{ path: '素材.psd', sizeBytes: null }]
			}
		});
	});

	it('空の検査結果は保存済みとして扱う', () => {
		expect(
			mapSaveResult({ commit: null, message: null, size_check: { blocked: [], warned: [] } })
		).toEqual({ kind: 'saved' });
	});

	it('選択をバックエンドの形にする', () => {
		expect(toBackendSizeChoice({ acceptWarned: true, exclude: ['a.psd'] })).toEqual({
			accept_warned: true,
			exclude: ['a.psd']
		});
	});
});

describe('プロジェクトの変換', () => {
	const info: Parameters<typeof mapProject>[0] = {
		id: 'p',
		display_name: 'P',
		path: 'C:\\p',
		remote_url: 'https://github.com/me/p.git',
		owner: 'me',
		last_viewed_at: '2026-10-08T00:00:00Z',
		folder_missing: false,
		last_uploaded_at: '2026-10-07T12:14:00+09:00',
		last_saved_at: '2026-10-08T09:30:00+09:00'
	};
	const status = {
		unsaved_changes: 0,
		upload_pending: 0,
		pull_pending: 0,
		has_conflicts: false,
		is_syncing: false,
		interrupted_operation: null,
		folder_missing: false,
		watching: false,
		last_auto_snapshot_at: null
	};

	it('最終保存の日時は、一覧の情報の値（自動保存を含まない最新の保存）から出す', () => {
		expect(mapProject(info, status).lastSavedAt?.toISOString()).toBe('2026-10-08T00:30:00.000Z');
		expect(mapProject({ ...info, last_saved_at: null }, status).lastSavedAt).toBeNull();
	});

	it('途中で止まった操作の名前を引き継ぐ', () => {
		expect(mapProject(info, { ...status, interrupted_operation: 'pull' })).toMatchObject({
			interruptedOperation: 'pull'
		});
		expect(mapProject(info, status).interruptedOperation).toBeNull();
	});

	it('最終アップロード日時と接続の有無を、実際の値から出す', () => {
		const connected = mapProject(info, status);
		expect(connected.remoteConnected).toBe(true);
		expect(connected.lastUploadedAt?.toISOString()).toBe('2026-10-07T03:14:00.000Z');

		const local = mapProject({ ...info, remote_url: null, last_uploaded_at: null }, status);
		expect(local.remoteConnected).toBe(false);
		expect(local.lastUploadedAt).toBeNull();
		expect(mapProject({ ...info, remote_url: '' }, status).remoteConnected).toBe(false);
	});

	it('ファイル監視の状態と、最後の自動保存の時刻を引き継ぐ', () => {
		const idle = mapProject(info, status);
		expect(idle.watching).toBe(false);
		expect(idle.lastAutoSnapshotAt).toBeNull();
		const watched = mapProject(info, {
			...status,
			watching: true,
			last_auto_snapshot_at: '2026-10-08T03:00:00Z'
		});
		expect(watched.watching).toBe(true);
		expect(watched.lastAutoSnapshotAt?.toISOString()).toBe('2026-10-08T03:00:00.000Z');
	});

	it('フォルダが見つからない状態を、一覧の情報と状態のどちらからでも拾う', () => {
		expect(mapProject(info, status).folderMissing).toBe(false);
		expect(mapProject({ ...info, folder_missing: true }, status).folderMissing).toBe(true);
		expect(mapProject(info, { ...status, folder_missing: true }).folderMissing).toBe(true);
	});
});

describe('保存の履歴の変換', () => {
	const item = {
		commit: 'abc1234',
		timestamp: '2026-10-07T21:14:00+09:00',
		message: 'm',
		changed_files_count: 1,
		is_snapshot: false,
		pc_name: null,
		cloud_synced: true
	};

	it('クラウドに上がっているかを、そのまま引き継ぐ', () => {
		expect(mapSavePoint(item).cloudSynced).toBe(true);
		expect(mapSavePoint({ ...item, cloud_synced: false }).cloudSynced).toBe(false);
	});
});

describe('保存先の作成・接続の結果の変換', () => {
	const base = {
		repository: 'me/p',
		connected: true,
		uploaded: true,
		size_check: null,
		existing_empty_repository: null,
		error: null
	};

	it('成功した結果をそのまま引き継ぐ', () => {
		expect(mapRemoteOutcome(base)).toEqual({
			repository: 'me/p',
			connected: true,
			uploaded: true,
			sizeCheck: null,
			existingEmptyRepository: null,
			error: null
		});
	});

	it('同名の空の保存先がある結果は、何も接続していない状態で名前を引き継ぐ', () => {
		const outcome = mapRemoteOutcome({
			...base,
			repository: null,
			connected: false,
			uploaded: false,
			existing_empty_repository: 'me/p'
		});
		expect(outcome).toMatchObject({
			connected: false,
			repository: null,
			existingEmptyRepository: 'me/p',
			error: null
		});
	});

	it('途中で失敗したときは、3 要素の文言を持つエラーにする（データの安否と次の行動を含む）', () => {
		const outcome = mapRemoteOutcome({
			...base,
			repository: null,
			connected: false,
			uploaded: false,
			error: {
				code: 'remote_name_taken',
				params: [],
				what_happened: '同じ名前の保存先がすでに GitHub にあります。',
				data_is_safe: 'ファイルはこの PC に安全に残っています。',
				next_action: '別の名前を指定して、もう一度お試しください',
				technical_info: null
			}
		});
		expect(outcome.connected).toBe(false);
		expect(outcome.error?.backend).toMatchObject({
			code: 'remote_name_taken',
			whatHappened: '同じ名前の保存先がすでに GitHub にあります。',
			dataIsSafe: 'ファイルはこの PC に安全に残っています。',
			nextAction: '別の名前を指定して、もう一度お試しください'
		});
	});

	it('大きいファイルで最初の保存を見送った場合の内容を引き継ぐ', () => {
		const outcome = mapRemoteOutcome({
			...base,
			uploaded: false,
			size_check: { blocked: [], warned: [{ path: 'a.psd', size: 72 }] }
		});
		expect(outcome.sizeCheck?.warned).toEqual([{ path: 'a.psd', sizeBytes: 72 }]);
	});
});

describe('差分の取得結果の変換', () => {
	const none = { size: null, modified_at: null };

	it('新規ファイルの差分は、全行が追加の行になる（行番号は 1 から）', () => {
		const diff = mapDiffResult(
			{
				kind: 'lines',
				lines: [
					{ kind: 'added', content: '一行目' },
					{ kind: 'added', content: '二行目' }
				],
				...none
			},
			'資料/新しい.txt'
		);
		expect(diff).toEqual({
			kind: 'text',
			rows: [
				{ kind: 'add', oldNo: null, newNo: 1, text: '一行目' },
				{ kind: 'add', oldNo: null, newNo: 2, text: '二行目' }
			]
		});
	});

	it('差分の行が無いときは「同一」、空の新規ファイルは専用の状態にする', () => {
		expect(mapDiffResult({ kind: 'lines', lines: [], ...none }, 'a.txt')).toEqual({
			kind: 'identical'
		});
		expect(mapDiffResult({ kind: 'new-file-empty', lines: [], ...none }, 'a.txt')).toEqual({
			kind: 'new_empty'
		});
	});

	it('バイナリはファイルの種類・サイズ・更新日時だけ、大きすぎるものはサイズだけにする', () => {
		const info = mapDiffResult(
			{ kind: 'binary', lines: [], size: 184_320, modified_at: 1_700_000_000 },
			'図4.png'
		);
		expect(info).toMatchObject({ kind: 'info', fileKind: 'image', sizeBytes: 184_320 });
		expect(info.kind === 'info' && info.modifiedAt?.getTime()).toBe(1_700_000_000_000);

		const unknownTime = mapDiffResult(
			{ kind: 'binary', lines: [], size: 8, modified_at: null },
			'data.bin'
		);
		expect(unknownTime).toMatchObject({ kind: 'info', fileKind: 'unknown', modifiedAt: null });

		expect(
			mapDiffResult({ kind: 'too-large', lines: [], size: 2_000_000, modified_at: 1 }, 'big.txt')
		).toEqual({ kind: 'too_large', sizeBytes: 2_000_000 });
	});
});

describe('プロジェクトのファイル一覧の変換', () => {
	it('サイズ・更新日時・変更の種類を引き継ぎ、削除されたファイルはサイズを持たない', () => {
		const tree = mapProjectTree({
			entries: [
				{
					path: '資料/第3章.docx',
					size: 100,
					modified_at: 1_700_000_000,
					change: 'modified',
					conflicted: false
				},
				{ path: '古い案.txt', size: null, modified_at: null, change: 'deleted', conflicted: false },
				{ path: 'メモ.txt', size: 5, modified_at: 1, change: null, conflicted: true }
			],
			truncated: true,
			limit: 10_000
		});
		expect(tree.truncated).toBe(true);
		expect(tree.limit).toBe(10_000);
		expect(tree.files[0]).toMatchObject({
			path: '資料/第3章.docx',
			sizeBytes: 100,
			change: 'modified',
			isConflict: false
		});
		expect(tree.files[0].modifiedAt?.getTime()).toBe(1_700_000_000_000);
		expect(tree.files[1]).toMatchObject({
			sizeBytes: null,
			modifiedAt: null,
			change: 'deleted'
		});
		expect(tree.files[2]).toMatchObject({ change: null, isConflict: true });
	});

	it('パスの区切りを / に統一する', () => {
		const tree = mapProjectTree({
			entries: [{ path: 'a\\b.txt', size: 1, modified_at: 1, change: null, conflicted: false }],
			truncated: false,
			limit: 10_000
		});
		expect(tree.files[0].path).toBe('a/b.txt');
	});
});
