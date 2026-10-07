import { describe, it, expect } from 'vitest';
import type { SavePoint } from '#lib/api/types.js';
import { canStep, defaultPoints, pointOptions, stepBlock, swapPoints } from './navigation';

function point(id: string, hoursAgo: number, kind: SavePoint['kind'] = 'save'): SavePoint {
	return {
		id,
		createdAt: new Date(Date.now() - hoursAgo * 3_600_000),
		message: kind === 'save' ? id : '',
		kind,
		cloudSynced: false
	};
}

describe('差分箇所の移動', () => {
	it('未選択から「次へ」で最初、「前へ」で最後の箇所に移る', () => {
		expect(stepBlock(null, 3, 1)).toBe(0);
		expect(stepBlock(null, 3, -1)).toBe(2);
	});

	it('端を越えて移動しない', () => {
		expect(stepBlock(2, 3, 1)).toBe(2);
		expect(stepBlock(0, 3, -1)).toBe(0);
		expect(stepBlock(0, 3, 1)).toBe(1);
	});

	it('差分が無いときは移動できない', () => {
		expect(stepBlock(null, 0, 1)).toBeNull();
		expect(canStep(null, 0, 1)).toBe(false);
	});

	it('端では該当方向のボタンを無効にできる', () => {
		expect(canStep(0, 3, -1)).toBe(false);
		expect(canStep(0, 3, 1)).toBe(true);
		expect(canStep(2, 3, 1)).toBe(false);
		expect(canStep(2, 3, -1)).toBe(true);
	});
});

describe('時点の選択', () => {
	it('入れ替えると比較元と比較先が逆になる', () => {
		expect(swapPoints('sp1', 'current')).toEqual({ from: 'current', to: 'sp1' });
	});

	it('選択肢は「いま」が先頭で、以降は新しい順に並ぶ', () => {
		const options = pointOptions([point('old', 48), point('new', 1), point('mid', 24, 'auto')]);
		expect(options.map((o) => o.value)).toEqual(['current', 'new', 'mid', 'old']);
		expect(options[2].description).toBeTruthy();
	});

	it('既定の比較元は直前の保存ポイントで、自動保存は選ばれない', () => {
		const points = [point('auto', 1, 'auto'), point('save', 5), point('older', 30)];
		expect(defaultPoints(points)).toEqual({ from: 'save', to: 'current' });
	});

	it('保存ポイントが無いときは「いま」同士になる', () => {
		expect(defaultPoints([])).toEqual({ from: 'current', to: 'current' });
	});
});
