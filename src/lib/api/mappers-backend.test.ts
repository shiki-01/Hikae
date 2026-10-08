import { describe, it, expect } from 'vitest';
import { describeError } from '#lib/features/notifications/error-view.js';
import {
	mapAddFilesResult,
	mapDeviceFlow,
	mapFileImpact,
	mapOverridden,
	mapOwner,
	mapPointChange,
	mapRemoteProjects,
	mapSession,
	mapSettings,
	mapSettingsView,
	restoreFileFailure,
	toBackendPatch
} from './mappers';

describe('ログインの変換', () => {
	it('ユーザー名だけを取り出し、トークンに相当する値は持たない', () => {
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
			userLogin: 'shiki'
		});
	});

	it('未ログインではユーザー名が無い', () => {
		const session = mapSession({
			logged_in: false,
			reauth_required: false,
			onboarded: false,
			user: null
		});
		expect(session.userLogin).toBeNull();
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
		expect(toBackendPatch({ pullIntervalMinutes: 0, autoSnapshotEnabled: false })).toEqual({
			pull_interval_minutes: 0,
			auto_snapshot_enabled: false
		});
		expect(toBackendPatch({})).toEqual({});
	});

	it('false や 0 も省略せず送る', () => {
		expect(toBackendPatch({ saveBeforePull: false })).toEqual({ save_before_pull: false });
	});
});
