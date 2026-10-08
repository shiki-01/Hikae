import { describe, it, expect } from 'vitest';
import type { Project } from '#lib/api/types.js';
import { cardStatus, needsAttention, needsConnection } from './project-card';
import { sortProjects } from '#lib/features/projects/sort.js';

function project(overrides: Partial<Project> = {}): Project {
	return {
		id: 'p',
		name: 'p',
		path: 'C:\\p',
		ownerName: 'me',
		ownerKind: 'personal',
		lastSavedAt: new Date('2026-10-01T00:00:00Z'),
		remoteConnected: true,
		lastUploadedAt: null,
		unsavedCount: 0,
		uploadPendingCount: 0,
		fetchPendingCount: 0,
		hasConflict: false,
		folderMissing: false,
		interruptedOperation: null,
		watching: false,
		lastAutoSnapshotAt: null,
		...overrides
	};
}

describe('プロジェクトカードの状態', () => {
	it('フォルダ不明は他の状態より優先される', () => {
		expect(cardStatus(project({ folderMissing: true, hasConflict: true }))).toBe('folder_missing');
	});

	it('ぶつかり、未保存、取り込み待ち、アップロード待ち、保存済みの順に判定する', () => {
		expect(cardStatus(project({ hasConflict: true, unsavedCount: 2 }))).toBe('conflict');
		expect(cardStatus(project({ unsavedCount: 2, fetchPendingCount: 1 }))).toBe('unsaved');
		expect(cardStatus(project({ fetchPendingCount: 1, uploadPendingCount: 1 }))).toBe(
			'fetch_pending'
		);
		expect(cardStatus(project({ uploadPendingCount: 1 }))).toBe('push_pending');
		expect(cardStatus(project())).toBe('saved');
	});

	it('途中で止まった操作は、ぶつかり以外の状態より優先して要対応にする', () => {
		const stopped = project({
			interruptedOperation: 'save',
			unsavedCount: 2,
			uploadPendingCount: 1
		});
		expect(cardStatus(stopped)).toBe('interrupted');
		expect(needsAttention(stopped)).toBe(true);
		expect(cardStatus(project({ interruptedOperation: 'pull', hasConflict: true }))).toBe(
			'conflict'
		);
	});

	it('要対応はぶつかりとフォルダ不明', () => {
		expect(needsAttention(project({ hasConflict: true }))).toBe(true);
		expect(needsAttention(project({ folderMissing: true }))).toBe(true);
		expect(needsAttention(project({ unsavedCount: 3 }))).toBe(false);
	});
});

describe('プロジェクト一覧の並び順', () => {
	it('要対応のカードを先頭に寄せ、それ以外は最終保存が新しい順に並べる', () => {
		const sorted = sortProjects([
			project({ id: 'old', lastSavedAt: new Date('2026-09-01T00:00:00Z') }),
			project({ id: 'conflict', hasConflict: true, lastSavedAt: new Date('2026-08-01T00:00:00Z') }),
			project({ id: 'new', lastSavedAt: new Date('2026-10-05T00:00:00Z') }),
			project({ id: 'never', lastSavedAt: null })
		]);
		expect(sorted.map((p) => p.id)).toEqual(['conflict', 'new', 'old', 'never']);
	});
});

describe('GitHub への接続の案内', () => {
	it('接続していないプロジェクトに案内を出す', () => {
		expect(needsConnection(project({ remoteConnected: false }))).toBe(true);
		expect(needsConnection(project({ remoteConnected: true }))).toBe(false);
	});

	it('フォルダが見つからないときは、先にフォルダの対応が必要なので案内しない', () => {
		expect(needsConnection(project({ remoteConnected: false, folderMissing: true }))).toBe(false);
	});

	it('接続していないだけでは要対応にしない（カードの状態も保存済みのまま）', () => {
		const local = project({ remoteConnected: false });
		expect(needsAttention(local)).toBe(false);
		expect(cardStatus(local)).toBe('saved');
	});
});
