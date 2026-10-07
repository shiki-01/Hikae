import type { Choice, ConflictFile, ConflictResolution } from '#lib/api/types.js';

export interface SelectionEntry {
	choice: Choice | null;
	keepBoth: boolean;
}

export type Selection = Record<string, SelectionEntry>;

export function initSelection(conflicts: ConflictFile[]): Selection {
	const selection: Selection = {};
	for (const conflict of conflicts) {
		selection[conflict.path] = { choice: null, keepBoth: conflict.kind === 'both' };
	}
	return selection;
}

export function setChoice(selection: Selection, path: string, choice: Choice): Selection {
	const entry = selection[path];
	if (!entry) return selection;
	return { ...selection, [path]: { ...entry, choice } };
}

export function setKeepBoth(selection: Selection, path: string, keepBoth: boolean): Selection {
	const entry = selection[path];
	if (!entry) return selection;
	return { ...selection, [path]: { ...entry, keepBoth } };
}

export function unresolvedCount(conflicts: ConflictFile[], selection: Selection): number {
	return conflicts.filter((c) => selection[c.path]?.choice == null).length;
}

export function canResolve(conflicts: ConflictFile[], selection: Selection): boolean {
	return conflicts.length > 0 && unresolvedCount(conflicts, selection) === 0;
}

export function toResolutions(
	conflicts: ConflictFile[],
	selection: Selection
): ConflictResolution[] {
	const result: ConflictResolution[] = [];
	for (const conflict of conflicts) {
		const entry = selection[conflict.path];
		if (!entry || entry.choice === null) continue;
		result.push({
			path: conflict.path,
			choice: entry.choice,
			keepBoth: conflict.kind === 'both' && entry.keepBoth
		});
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
