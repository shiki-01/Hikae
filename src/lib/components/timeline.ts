import type { SavePoint, TimelineSnapshots } from '#lib/api/types.js';

export type TimelineEntry =
	| { kind: 'now'; id: 'now' }
	| { kind: 'save'; id: string; point: SavePoint }
	| { kind: 'autos'; id: string; points: SavePoint[] }
	| { kind: 'cloud_line'; id: 'cloud-line' };

/** 自動保存は、設定が「表示しない」なら除く。「折りたたむ」「表示する」の違いは表示側で扱う */
export function buildTimeline(
	points: SavePoint[],
	snapshots: TimelineSnapshots = 'collapsed'
): TimelineEntry[] {
	const visible = snapshots === 'hidden' ? points.filter((p) => p.kind !== 'auto') : points;
	const sorted = [...visible].sort((a, b) => b.createdAt.getTime() - a.createdAt.getTime());
	const entries: TimelineEntry[] = [{ kind: 'now', id: 'now' }];
	let lineInserted = false;

	for (const point of sorted) {
		if (!lineInserted && point.kind === 'save' && point.cloudSynced) {
			entries.push({ kind: 'cloud_line', id: 'cloud-line' });
			lineInserted = true;
		}
		if (point.kind === 'auto') {
			const last = entries[entries.length - 1];
			if (last.kind === 'autos') {
				last.points.push(point);
			} else {
				entries.push({ kind: 'autos', id: `autos-${point.id}`, points: [point] });
			}
		} else {
			entries.push({ kind: 'save', id: point.id, point });
		}
	}
	return entries;
}

export function latestManualPoint(points: SavePoint[]): SavePoint | undefined {
	return points
		.filter((p) => p.kind === 'save')
		.sort((a, b) => b.createdAt.getTime() - a.createdAt.getTime())[0];
}
