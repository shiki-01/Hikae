import { describe, expect, it } from 'vitest';
import type { SavePoint } from '#lib/api/types.js';
import { mockApi } from '#lib/api/mock.js';
import { flattenHistoryPages, nextHistoryOffset } from './paging';

function point(id: string): SavePoint {
	return { id, createdAt: new Date(0), message: id, kind: 'save', cloudSynced: false };
}

function page(from: number, count: number): SavePoint[] {
	return Array.from({ length: count }, (_, i) => point(`p${from + i}`));
}

describe('履歴のページ送り', () => {
	it('ページをつなげると取得した順に並ぶ', () => {
		const joined = flattenHistoryPages([page(0, 3), page(3, 2)]);
		expect(joined.map((p) => p.id)).toEqual(['p0', 'p1', 'p2', 'p3', 'p4']);
		expect(flattenHistoryPages([])).toEqual([]);
	});

	it('ページの境目で同じ項目が重なっても、1 件にまとめる', () => {
		const joined = flattenHistoryPages([page(0, 3), page(2, 3)]);
		expect(joined.map((p) => p.id)).toEqual(['p0', 'p1', 'p2', 'p3', 'p4']);
	});

	it('最後のページが満たなければ、次のページは無い', () => {
		expect(nextHistoryOffset([], 3)).toBeUndefined();
		expect(nextHistoryOffset([page(0, 3), page(3, 2)], 3)).toBeUndefined();
		expect(nextHistoryOffset([page(0, 3)], 3)).toBe(3);
	});

	it('次の開始位置は、重複を除く前の取得件数で数える', () => {
		expect(nextHistoryOffset([page(0, 3), page(2, 3)], 3)).toBe(6);
	});

	it('モックも offset と limit で履歴を切り出し、ページをつなげると全体になる', async () => {
		const all = await mockApi.listSavePoints('proj1', 0, 1000);
		expect(all.length).toBeGreaterThan(1);
		const first = await mockApi.listSavePoints('proj1', 0, 1);
		const rest = await mockApi.listSavePoints('proj1', 1, 1000);
		expect([...first, ...rest].map((p) => p.id)).toEqual(all.map((p) => p.id));
		expect(await mockApi.listSavePoints('proj1', all.length, 10)).toEqual([]);
	});
});
