import type { DiffRow } from './types';

export function diffLines(before: string[], after: string[]): DiffRow[] {
	const n = before.length;
	const m = after.length;
	const table: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
	for (let i = n - 1; i >= 0; i--) {
		for (let j = m - 1; j >= 0; j--) {
			table[i][j] =
				before[i] === after[j]
					? table[i + 1][j + 1] + 1
					: Math.max(table[i + 1][j], table[i][j + 1]);
		}
	}

	const rows: DiffRow[] = [];
	let i = 0;
	let j = 0;
	while (i < n && j < m) {
		if (before[i] === after[j]) {
			rows.push({ kind: 'context', oldNo: i + 1, newNo: j + 1, text: before[i] });
			i++;
			j++;
		} else if (table[i + 1][j] >= table[i][j + 1]) {
			rows.push({ kind: 'del', oldNo: i + 1, newNo: null, text: before[i] });
			i++;
		} else {
			rows.push({ kind: 'add', oldNo: null, newNo: j + 1, text: after[j] });
			j++;
		}
	}
	while (i < n) {
		rows.push({ kind: 'del', oldNo: i + 1, newNo: null, text: before[i] });
		i++;
	}
	while (j < m) {
		rows.push({ kind: 'add', oldNo: null, newNo: j + 1, text: after[j] });
		j++;
	}
	return rows;
}
