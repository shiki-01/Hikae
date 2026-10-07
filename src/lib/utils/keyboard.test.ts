import { describe, it, expect } from 'vitest';
import { nextIndex } from './keyboard';

describe('矢印キーによるフォーカス移動', () => {
	it('先頭と末尾で循環する', () => {
		expect(nextIndex('ArrowDown', 2, 3)).toBe(0);
		expect(nextIndex('ArrowUp', 0, 3)).toBe(2);
	});

	it('Home と End で両端に移る', () => {
		expect(nextIndex('Home', 2, 5)).toBe(0);
		expect(nextIndex('End', 0, 5)).toBe(4);
	});

	it('向きの指定に合わない矢印キーは無視する', () => {
		expect(nextIndex('ArrowRight', 0, 3, 'vertical')).toBeNull();
		expect(nextIndex('ArrowDown', 0, 3, 'horizontal')).toBeNull();
		expect(nextIndex('ArrowRight', 0, 3, 'horizontal')).toBe(1);
	});

	it('対象外のキーや空の一覧では何もしない', () => {
		expect(nextIndex('a', 0, 3)).toBeNull();
		expect(nextIndex('ArrowDown', 0, 0)).toBeNull();
	});
});
