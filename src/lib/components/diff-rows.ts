import type { DiffRow } from '#lib/api/types.js';

export interface BlockRow {
	row: DiffRow;
	block: number | null;
}

export interface SplitRow {
	left: DiffRow | null;
	right: DiffRow | null;
	block: number | null;
}

export function annotateBlocks(rows: DiffRow[]): BlockRow[] {
	let block = -1;
	let inBlock = false;
	return rows.map((row) => {
		if (row.kind === 'context') {
			inBlock = false;
			return { row, block: null };
		}
		if (!inBlock) {
			block += 1;
			inBlock = true;
		}
		return { row, block };
	});
}

export function countBlocks(rows: DiffRow[]): number {
	let max = -1;
	for (const item of annotateBlocks(rows)) {
		if (item.block !== null && item.block > max) max = item.block;
	}
	return max + 1;
}

export function pairRows(rows: DiffRow[]): SplitRow[] {
	const annotated = annotateBlocks(rows);
	const result: SplitRow[] = [];
	let index = 0;
	while (index < annotated.length) {
		const current = annotated[index];
		if (current.row.kind === 'context') {
			result.push({ left: current.row, right: current.row, block: null });
			index += 1;
			continue;
		}
		const dels: DiffRow[] = [];
		const adds: DiffRow[] = [];
		const block = current.block;
		while (index < annotated.length && annotated[index].block === block) {
			const { row } = annotated[index];
			if (row.kind === 'del') dels.push(row);
			else adds.push(row);
			index += 1;
		}
		const length = Math.max(dels.length, adds.length);
		for (let i = 0; i < length; i++) {
			result.push({ left: dels[i] ?? null, right: adds[i] ?? null, block });
		}
	}
	return result;
}
