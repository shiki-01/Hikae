import { describe, it, expect } from 'vitest';
import type { Change } from '#lib/api/types.js';
import { actionsFor } from './change-actions';

function change(overrides: Partial<Change>): Change {
	return { id: 'a.txt', path: 'a.txt', type: 'added', isConflict: false, ...overrides };
}

describe('actionsFor', () => {
	it('未追跡の新規ファイルだけが「元に戻す（作成しない）」になる', () => {
		expect(actionsFor(change({ untracked: true })).discard).toBe(true);
	});

	it('登録済みの新規ファイル・変更・削除・名前変更は、従来の「元に戻す」のまま', () => {
		expect(actionsFor(change({ untracked: false })).discard).toBe(false);
		expect(actionsFor(change({})).discard).toBe(false);
		for (const type of ['modified', 'deleted', 'renamed'] as const) {
			expect(actionsFor(change({ type, untracked: true })).discard).toBe(false);
		}
	});

	it('ぶつかり中のファイルは対象にしない', () => {
		expect(actionsFor(change({ untracked: true, isConflict: true })).discard).toBe(false);
	});
});
