import { describe, expect, it } from 'vitest';
import { checkAddable, MAX_FILE_BYTES, MAX_ITEMS, MAX_TOTAL_BYTES } from './limits';

const MiB = 1024 * 1024;

describe('追加の上限', () => {
	it('1 ファイル 25 MiB までは追加できる', () => {
		expect(checkAddable([], [{ name: 'a', size: MAX_FILE_BYTES }])).toBeNull();
	});

	it('1 ファイルが 25 MiB を超えると、そのファイルを理由にして追加しない', () => {
		expect(checkAddable([], [{ name: 'big.mov', size: MAX_FILE_BYTES + 1 }])).toEqual({
			kind: 'file',
			name: 'big.mov',
			limit: MAX_FILE_BYTES
		});
	});

	it('合計が 60 MiB を超えると追加しない（既に積んだ分を含む）', () => {
		const current = [{ size: 30 * MiB }];
		const incoming = [
			{ name: 'b', size: 25 * MiB },
			{ name: 'c', size: 10 * MiB }
		];
		expect(checkAddable(current, incoming)).toEqual({ kind: 'total', limit: MAX_TOTAL_BYTES });
	});

	it('合計ちょうどの 60 MiB までは追加できる', () => {
		expect(checkAddable([{ size: 35 * MiB }], [{ name: 'b', size: 25 * MiB }])).toBeNull();
	});

	it('100 件を超えると追加しない', () => {
		const current = Array.from({ length: MAX_ITEMS }, () => ({ size: 0 }));
		expect(checkAddable(current, [{ name: 'x', size: 0 }])).toEqual({
			kind: 'count',
			limit: MAX_ITEMS
		});
		expect(checkAddable(current.slice(1), [{ name: 'x', size: 0 }])).toBeNull();
	});
});
