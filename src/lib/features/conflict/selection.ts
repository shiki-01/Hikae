import type { Choice, ConflictEdit, ConflictFile, ConflictResolution } from '#lib/api/types.js';

export interface SelectionEntry {
	choice: Choice | null;
	keepBoth: boolean;
	/** 行ごとに編集して確定した解消後の全文。null のときは choice で決める */
	edit: string | null;
}

export type Selection = Record<string, SelectionEntry>;

export function initSelection(conflicts: ConflictFile[]): Selection {
	const selection: Selection = {};
	for (const conflict of conflicts) {
		selection[conflict.path] = { choice: null, keepBoth: conflict.kind === 'both', edit: null };
	}
	return selection;
}

/** 2 択で選ぶと、行ごとの編集は取り消される（どちらか一方の決め方だけを残す） */
export function setChoice(selection: Selection, path: string, choice: Choice): Selection {
	const entry = selection[path];
	if (!entry) return selection;
	return { ...selection, [path]: { ...entry, choice, edit: null } };
}

export function setKeepBoth(selection: Selection, path: string, keepBoth: boolean): Selection {
	const entry = selection[path];
	if (!entry) return selection;
	return { ...selection, [path]: { ...entry, keepBoth } };
}

/** 行ごとの編集の確定（null で取り消し）。確定したときは 2 択の選択を取り消す */
export function setEdit(selection: Selection, path: string, edit: string | null): Selection {
	const entry = selection[path];
	if (!entry) return selection;
	return { ...selection, [path]: { ...entry, choice: null, edit } };
}

/** 決まっているファイル（2 択、または行ごとの編集の確定）の数を除いた残り */
export function unresolvedCount(conflicts: ConflictFile[], selection: Selection): number {
	return conflicts.filter((c) => !isDecided(selection[c.path])).length;
}

function isDecided(entry: SelectionEntry | undefined): boolean {
	return entry !== undefined && (entry.choice !== null || entry.edit !== null);
}

export function canResolve(conflicts: ConflictFile[], selection: Selection): boolean {
	return conflicts.length > 0 && unresolvedCount(conflicts, selection) === 0;
}

/**
 * 解消の指示（choices）。行ごとに編集したファイルも含め、決まっている全ファイルを 1 件ずつ入れる。
 * 編集したファイルの choice は backend で無視されるが、全件を覆うために 'this' を入れる。
 */
export function toResolutions(
	conflicts: ConflictFile[],
	selection: Selection
): ConflictResolution[] {
	const result: ConflictResolution[] = [];
	for (const conflict of conflicts) {
		const entry = selection[conflict.path];
		if (!entry || !isDecided(entry)) continue;
		if (entry.edit !== null) {
			result.push({ path: conflict.path, choice: 'this', keepBoth: false });
			continue;
		}
		if (entry.choice === null) continue;
		result.push({
			path: conflict.path,
			choice: entry.choice,
			keepBoth: conflict.kind === 'both' && entry.keepBoth
		});
	}
	return result;
}

/** 行ごとに編集して確定したファイルの全文（解消の指示の edits） */
export function toEdits(conflicts: ConflictFile[], selection: Selection): ConflictEdit[] {
	const result: ConflictEdit[] = [];
	for (const conflict of conflicts) {
		const entry = selection[conflict.path];
		if (entry?.edit != null) result.push({ path: conflict.path, text: entry.edit });
	}
	return result;
}

export function syncSelection(conflicts: ConflictFile[], selection: Selection): Selection {
	const initial = initSelection(conflicts);
	const next: Selection = {};
	for (const path of Object.keys(initial)) {
		next[path] = selection[path] ?? initial[path];
	}
	const sameKeys =
		Object.keys(next).length === Object.keys(selection).length &&
		Object.keys(next).every((path) => path in selection);
	return sameKeys ? selection : next;
}
