import { describe, it, expect } from 'vitest';
import type { Project } from '#lib/api/types.js';
import { cardStatus, needsAttention } from './project-card';
import { sortProjects } from '#lib/features/projects/sort.js';

function project(overrides: Partial<Project> = {}): Project {
	return {
		id: 'p',
		name: 'p',
		path: 'C:\\p',
		ownerName: 'me',
		ownerKind: 'personal',
		lastSavedAt: new Date('2026-10-01T00:00:00Z'),
		lastUploadedAt: null,
		unsavedCount: 0,
		uploadPendingCount: 0,
		fetchPendingCount: 0,
		hasConflict: false,
		folderMissing: false,
		interruptedOperation: null,
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
