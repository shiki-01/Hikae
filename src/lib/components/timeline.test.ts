import { describe, it, expect } from 'vitest';
import type { SavePoint } from '#lib/api/types.js';
import { buildTimeline, latestManualPoint } from './timeline';

function point(
	id: string,
	hoursAgo: number,
	kind: SavePoint['kind'],
	cloudSynced = false
): SavePoint {
	return {
		id,
		createdAt: new Date(Date.UTC(2026, 9, 7, 12) - hoursAgo * 3_600_000),
		message: kind === 'save' ? id : '',
		kind,
		cloudSynced
	};
}

describe('タイムラインの組み立て', () => {
	it('先頭は「いま」で、保存ポイントは新しい順に並ぶ', () => {
		const entries = buildTimeline([point('b', 10, 'save'), point('a', 1, 'save')]);
		expect(entries.map((e) => e.id)).toEqual(['now', 'a', 'b']);
	});

	it('連続する自動保存は1つにまとめる', () => {
		const entries = buildTimeline([
			point('s1', 1, 'save'),
			point('a1', 2, 'auto'),
			point('a2', 3, 'auto'),
			point('s2', 4, 'save'),
			point('a3', 5, 'auto')
		]);
		const kinds = entries.map((e) => e.kind);
		expect(kinds).toEqual(['now', 'save', 'autos', 'save', 'autos']);
		const first = entries[2];
		expect(first.kind === 'autos' && first.points.map((p) => p.id)).toEqual(['a1', 'a2']);
	});

	it('クラウド境界線は、アップロード済みの最初の保存ポイントの手前に1本だけ入る', () => {
		const entries = buildTimeline([
			point('s1', 1, 'save'),
			point('a1', 2, 'auto'),
			point('s2', 3, 'save', true),
			point('s3', 4, 'save', true)
		]);
		expect(entries.map((e) => e.kind)).toEqual([
			'now',
			'save',
			'autos',
			'cloud_line',
			'save',
			'save'
		]);
	});

	it('アップロード済みが無ければ境界線は出ない', () => {
		const entries = buildTimeline([point('s1', 1, 'save')]);
		expect(entries.some((e) => e.kind === 'cloud_line')).toBe(false);
	});

	it('自動保存はアップロード済みでも境界の判定に使わない', () => {
		const entries = buildTimeline([point('a1', 1, 'auto', true), point('s1', 2, 'save')]);
		expect(entries.some((e) => e.kind === 'cloud_line')).toBe(false);
	});

	it('直前の保存ポイントは自動保存を除いて最も新しいもの', () => {
		const latest = latestManualPoint([
			point('a', 1, 'auto'),
			point('s', 2, 'save'),
			point('old', 9, 'save')
		]);
		expect(latest?.id).toBe('s');
		expect(latestManualPoint([])).toBeUndefined();
	});
});
