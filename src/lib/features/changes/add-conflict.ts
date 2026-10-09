import type { AddConflictAction, AddConflictDecision, NameConflict } from '#lib/api/types.js';

/** 追加先のパスごとの選択 */
export type ConflictChoices = Record<string, AddConflictAction>;

/**
 * 同じ選択を、すべてに当てはめる。置き換えられないもの（大きいファイルなど）は「両方残す」にする
 * （どれも元のファイルは壊れない選択）。
 */
export function applyToAll(conflicts: NameConflict[], action: AddConflictAction): ConflictChoices {
	const choices: ConflictChoices = {};
	for (const conflict of conflicts) {
		choices[conflict.path] = action === 'replace' && !conflict.canReplace ? 'keep_both' : action;
	}
	return choices;
}

/** 最初の選択。上書きしない「両方残す」を既定にする */
export function defaultChoices(conflicts: NameConflict[]): ConflictChoices {
	return applyToAll(conflicts, 'keep_both');
}

/** 1 件の選択を変える（置き換えられないものを置き換えにはしない） */
export function choose(
	choices: ConflictChoices,
	conflicts: NameConflict[],
	path: string,
	action: AddConflictAction
): ConflictChoices {
	const conflict = conflicts.find((c) => c.path === path);
	if (!conflict) return choices;
	if (action === 'replace' && !conflict.canReplace) return choices;
	return { ...choices, [path]: action };
}

/** バックエンドへ送る選択。確認の一覧にあるファイルすべてについて、選択を添える */
export function toDecisions(
	conflicts: NameConflict[],
	choices: ConflictChoices
): AddConflictDecision[] {
	return conflicts.map((conflict) => ({
		path: conflict.path,
		action: choices[conflict.path] ?? 'keep_both'
	}));
}

/** 全部が同じ選択ならその選択、ばらばらなら null（「すべてに同じ選択を使う」の表示用） */
export function commonChoice(
	conflicts: NameConflict[],
	choices: ConflictChoices
): AddConflictAction | null {
	const first = conflicts[0] ? (choices[conflicts[0].path] ?? 'keep_both') : null;
	if (first === null) return null;
	return conflicts.every((c) => (choices[c.path] ?? 'keep_both') === first) ? first : null;
}

/** 画面に出すファイル名とフォルダ（追加先のパスから） */
export function splitPath(path: string): { name: string; folder: string } {
	const index = path.lastIndexOf('/');
	return index < 0
		? { name: path, folder: '' }
		: { name: path.slice(index + 1), folder: path.slice(0, index) };
}

/** すべてを追加しない選択か（確認ボタンの文言の出し分けに使う） */
export function allSkipped(conflicts: NameConflict[], choices: ConflictChoices): boolean {
	return conflicts.length > 0 && commonChoice(conflicts, choices) === 'skip';
}
