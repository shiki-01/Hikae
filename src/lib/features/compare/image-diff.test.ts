import { describe, it, expect } from 'vitest';
import {
	DIFF_THRESHOLD,
	diffPixels,
	isImagePath,
	parseHexColor,
	renderDiff,
	type Pixels
} from './image-diff';

/** 同じ色で塗った画素（幅 × 高さ） */
function solid(width: number, height: number, rgba: [number, number, number, number]): Pixels {
	const data = new Uint8ClampedArray(width * height * 4);
	for (let i = 0; i < width * height; i++) data.set(rgba, i * 4);
	return { data, width, height };
}

/** 指定した画素だけ値を変えたコピー（x, y の画素の c チャンネルを value に） */
function withPixel(base: Pixels, x: number, y: number, channel: number, value: number): Pixels {
	const data = new Uint8ClampedArray(base.data);
	data[(y * base.width + x) * 4 + channel] = value;
	return { data, width: base.width, height: base.height };
}

describe('diffPixels', () => {
	it('同一の画像は違う画素が 0 で、差分率も 0', () => {
		const a = solid(4, 3, [10, 20, 30, 255]);
		const b = solid(4, 3, [10, 20, 30, 255]);
		const diff = diffPixels(a, b);
		expect(diff.changed).toBe(0);
		expect(diff.total).toBe(12);
		expect(diff.rate).toBe(0);
		expect(diff.sizeMismatch).toBe(false);
		expect(Array.from(diff.mask).every((v) => v === 0)).toBe(true);
	});

	it('1 画素だけ違うとき、その画素だけが違い、差分率は 1 / 総数', () => {
		const a = solid(4, 3, [10, 20, 30, 255]);
		const b = withPixel(a, 2, 1, 0, 200);
		const diff = diffPixels(a, b);
		expect(diff.changed).toBe(1);
		expect(diff.mask[1 * 4 + 2]).toBe(1);
		expect(diff.rate).toBeCloseTo(1 / 12);
	});

	it('アルファだけ違う画素も違うとみなす', () => {
		const a = solid(2, 2, [0, 0, 0, 255]);
		const b = withPixel(a, 0, 0, 3, 128);
		const diff = diffPixels(a, b);
		expect(diff.changed).toBe(1);
		expect(diff.mask[0]).toBe(1);
	});

	it('しきい値ちょうどの差は違わず、しきい値を 1 超えると違う', () => {
		const a = solid(1, 1, [100, 100, 100, 255]);
		const atThreshold = withPixel(a, 0, 0, 0, 100 + DIFF_THRESHOLD);
		const overThreshold = withPixel(a, 0, 0, 0, 100 + DIFF_THRESHOLD + 1);
		expect(diffPixels(a, atThreshold).changed).toBe(0);
		expect(diffPixels(a, overThreshold).changed).toBe(1);
		// しきい値は引数で変えられる
		expect(diffPixels(a, atThreshold, 0).changed).toBe(1);
	});

	it('サイズが違うとき、はみ出す領域は違う画素として数え、キャンバスは両者の最大になる', () => {
		const a = solid(2, 2, [50, 50, 50, 255]);
		const b = solid(3, 1, [50, 50, 50, 255]);
		const diff = diffPixels(a, b);
		expect(diff.width).toBe(3);
		expect(diff.height).toBe(2);
		expect(diff.total).toBe(6);
		expect(diff.sizeMismatch).toBe(true);
		// 重なる範囲は (0,0)(1,0) だけで同じ色。(2,0)、(0,1)、(1,1)、(2,1) は片方にしか無いので違う
		expect(diff.changed).toBe(4);
		expect(diff.mask[0]).toBe(0);
		expect(diff.mask[1]).toBe(0);
		expect(diff.mask[2]).toBe(1);
		expect(diff.mask[3]).toBe(1);
		expect(diff.mask[4]).toBe(1);
		expect(diff.mask[5]).toBe(1);
	});

	it('寸法と data の長さが合わなければ RangeError', () => {
		const good = solid(2, 2, [0, 0, 0, 255]);
		const bad: Pixels = { data: new Uint8ClampedArray(4), width: 2, height: 2 };
		expect(() => diffPixels(good, bad)).toThrow(RangeError);
	});

	it('両方が空（0 × 0）なら差分率は 0', () => {
		const empty = solid(0, 0, [0, 0, 0, 0]);
		const diff = diffPixels(empty, empty);
		expect(diff.total).toBe(0);
		expect(diff.rate).toBe(0);
	});
});

describe('renderDiff', () => {
	it('違う画素は強調色で塗り、違わない画素は薄い灰色にする', () => {
		const a = solid(2, 1, [0, 0, 0, 255]);
		const b = withPixel(a, 1, 0, 0, 255);
		const diff = diffPixels(a, b);
		const out = renderDiff(diff, a, [255, 0, 0]);
		// 0 番目は同じ（黒を薄くした灰色）
		expect(out[0]).toBeGreaterThan(0);
		expect(out[0]).toBe(out[1]);
		expect(out[3]).toBe(255);
		// 1 番目は強調色
		expect(Array.from(out.slice(4, 8))).toEqual([255, 0, 0, 255]);
	});
});

describe('isImagePath', () => {
	it('対応する拡張子を大文字小文字を問わず認める', () => {
		for (const name of ['a.png', 'dir/b.JPG', 'c.jpeg', 'd.GIF', 'e.webp', 'f.Bmp']) {
			expect(isImagePath(name), name).toBe(true);
		}
	});

	it('svg・文書・拡張子なしは画像としない', () => {
		for (const name of ['icon.svg', 'memo.txt', 'sheet.xlsx', 'noext', 'dir.png/x', '.png']) {
			expect(isImagePath(name), name).toBe(false);
		}
	});
});

describe('parseHexColor', () => {
	it('#rrggbb を分け、それ以外は null', () => {
		expect(parseHexColor('#e8262a')).toEqual([0xe8, 0x26, 0x2a]);
		expect(parseHexColor(' #FF5A5F ')).toEqual([255, 0x5a, 0x5f]);
		expect(parseHexColor('red')).toBeNull();
		expect(parseHexColor('#fff')).toBeNull();
	});
});
