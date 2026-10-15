// 1 回に追加できる量の上限（docs/design.md 14.5）。画面のメモリと IPC の都合による暫定値。
// 超えるときは項目を追加せず、理由を示す（Rust 側でも同じ上限を検査する）。

export const MAX_FILE_BYTES = 25 * 1024 * 1024;
export const MAX_TOTAL_BYTES = 60 * 1024 * 1024;
export const MAX_ITEMS = 100;

export type LimitProblem =
	| { kind: 'file'; name: string; limit: number }
	| { kind: 'total'; limit: number }
	| { kind: 'count'; limit: number };

/**
 * 既に積んである項目（current）に、追加する項目（incoming）を足せるかを調べる。
 * 足せないときは最初に当たった理由を返す。足せるなら null。
 */
export function checkAddable(
	current: readonly { size: number }[],
	incoming: readonly { name: string; size: number }[]
): LimitProblem | null {
	const tooBig = incoming.find((item) => item.size > MAX_FILE_BYTES);
	if (tooBig) return { kind: 'file', name: tooBig.name, limit: MAX_FILE_BYTES };
	if (current.length + incoming.length > MAX_ITEMS) return { kind: 'count', limit: MAX_ITEMS };
	const total = [...current, ...incoming].reduce((sum, item) => sum + item.size, 0);
	if (total > MAX_TOTAL_BYTES) return { kind: 'total', limit: MAX_TOTAL_BYTES };
	return null;
}
