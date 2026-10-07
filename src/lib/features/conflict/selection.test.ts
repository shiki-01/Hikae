import { describe, it, expect } from 'vitest';
import type { ConflictFile } from '#lib/api/types.js';
import {
	canResolve,
	initSelection,
	setChoice,
	setKeepBoth,
	syncSelection,
	toResolutions,
	unresolvedCount
} from './selection';

function conflict(path: string, kind: ConflictFile['kind'] = 'both'): ConflictFile {
	const date = new Date('2026-10-06T10:00:00');
	return { path, kind, thisPcSavedAt: date, cloudSavedAt: date, cloudPcName: 'PC' };
}

describe('ぶつかり解消の選択状態', () => {
	const conflicts = [conflict('a.docx'), conflict('b.txt', 'deleted_in_cloud')];

	it('初期状態では全件が未選択で、解消できない', () => {
		const selection = initSelection(conflicts);
		expect(unresolvedCount(conflicts, selection)).toBe(2);
		expect(canResolve(conflicts, selection)).toBe(false);
	});

	it('別名で残すは両方変更型だけ初期オンになる', () => {
		const selection = initSelection(conflicts);
		expect(selection['a.docx'].keepBoth).toBe(true);
		expect(selection['b.txt'].keepBoth).toBe(false);
	});

	it('1件だけ選んでも、全件選ぶまで解消できない', () => {
		const selection = setChoice(initSelection(conflicts), 'a.docx', 'cloud');
		expect(unresolvedCount(conflicts, selection)).toBe(1);
		expect(canResolve(conflicts, selection)).toBe(false);
	});

	it('全件選ぶと解消できる', () => {
		let selection = initSelection(conflicts);
		selection = setChoice(selection, 'a.docx', 'cloud');
		selection = setChoice(selection, 'b.txt', 'this');
		expect(canResolve(conflicts, selection)).toBe(true);
	});

	it('ぶつかりが0件のときは解消できない', () => {
		expect(canResolve([], {})).toBe(false);
	});

	it('解消の指示には選択と別名保存の設定が入り、片方削除型は別名保存にならない', () => {
		let selection = initSelection(conflicts);
		selection = setChoice(selection, 'a.docx', 'cloud');
		selection = setKeepBoth(selection, 'a.docx', false);
		selection = setChoice(selection, 'b.txt', 'this');
		selection = setKeepBoth(selection, 'b.txt', true);
		expect(toResolutions(conflicts, selection)).toEqual([
			{ path: 'a.docx', choice: 'cloud', keepBoth: false },
			{ path: 'b.txt', choice: 'this', keepBoth: false }
		]);
	});

	it('未選択のファイルは解消の指示に含まれない', () => {
		const selection = setChoice(initSelection(conflicts), 'a.docx', 'this');
		expect(toResolutions(conflicts, selection).map((r) => r.path)).toEqual(['a.docx']);
	});

	it('再取得しても選択済みの内容は保たれ、増減だけ反映される', () => {
		const chosen = setChoice(initSelection(conflicts), 'a.docx', 'cloud');
		expect(syncSelection(conflicts, chosen)).toBe(chosen);
		const grown = syncSelection([...conflicts, conflict('c.txt')], chosen);
		expect(grown['a.docx'].choice).toBe('cloud');
		expect(grown['c.txt'].choice).toBeNull();
		const shrunk = syncSelection([conflicts[0]], chosen);
		expect(Object.keys(shrunk)).toEqual(['a.docx']);
	});
});
