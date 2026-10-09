// 画像比較の純関数（設計書 3.3 S4、デザイン要件 ImageCompare）。DOM と canvas に依存しない。
// 画素は RGBA の Uint8ClampedArray と寸法で受け取る（canvas の getImageData の結果をそのまま渡せる）。

/** 比較の対象になる拡張子（app-blob が返す mime と対応する。svg は含めない） */
export const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] as const;

/** 1 チャンネル（R・G・B・A のいずれか）の差がこの値を超えたら「違う画素」とみなす（小さな固定値） */
export const DIFF_THRESHOLD = 8;

/** RGBA の画素と寸法 */
export interface Pixels {
	data: Uint8ClampedArray;
	width: number;
	height: number;
}

/** 2 枚の画素の比較結果。寸法は両者の最大（キャンバスの大きさ） */
export interface PixelDiff {
	width: number;
	height: number;
	/** width * height 個。1 は違う画素（はみ出す領域を含む）、0 は同じ画素 */
	mask: Uint8Array;
	changed: number;
	total: number;
	/** 0〜1。total が 0 のときは 0 */
	rate: number;
	/** 寸法が両者で違うか */
	sizeMismatch: boolean;
}

/** パスの拡張子で画像として比較できるか（大文字小文字は区別しない） */
export function isImagePath(path: string): boolean {
	// ファイル名だけを見る。先頭の `.` だけの名前（`.png`）は拡張子なしとして扱う（Rust 側の Path::extension と同じ）
	const name = path.split(/[\\/]/).pop() ?? '';
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return false;
	const ext = name.slice(dot + 1).toLowerCase();
	return (IMAGE_EXTENSIONS as readonly string[]).includes(ext);
}

/**
 * 2 枚の画素を比べる。画素ごとに R・G・B・A の差の最大が `threshold` を超えたら違うとみなす。
 * 寸法が違うときは、片方しかない領域の画素をすべて違うとみなす（キャンバスの大きさは両者の最大）。
 * `data` の長さが寸法と合わないときは RangeError。
 */
export function diffPixels(a: Pixels, b: Pixels, threshold = DIFF_THRESHOLD): PixelDiff {
	assertShape(a, 'a');
	assertShape(b, 'b');
	const width = Math.max(a.width, b.width);
	const height = Math.max(a.height, b.height);
	const total = width * height;
	const mask = new Uint8Array(total);
	let changed = 0;

	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			const index = y * width + x;
			const inA = x < a.width && y < a.height;
			const inB = x < b.width && y < b.height;
			let differs: boolean;
			if (!inA || !inB) {
				differs = true;
			} else {
				differs = maxChannelDelta(a, b, x, y) > threshold;
			}
			if (differs) {
				mask[index] = 1;
				changed++;
			}
		}
	}

	return {
		width,
		height,
		mask,
		changed,
		total,
		rate: total > 0 ? changed / total : 0,
		sizeMismatch: a.width !== b.width || a.height !== b.height
	};
}

/**
 * 差分の表示用の画素（RGBA）を作る。違う画素は `highlight` の色で塗り、
 * そのほかは `a` の画素を薄くした色にする（`a` の外は違う画素なので参照しない）。
 */
export function renderDiff(
	diff: PixelDiff,
	a: Pixels,
	highlight: readonly [number, number, number]
): Uint8ClampedArray {
	const out = new Uint8ClampedArray(diff.width * diff.height * 4);
	for (let y = 0; y < diff.height; y++) {
		for (let x = 0; x < diff.width; x++) {
			const index = y * diff.width + x;
			const o = index * 4;
			if (diff.mask[index] === 1) {
				out[o] = highlight[0];
				out[o + 1] = highlight[1];
				out[o + 2] = highlight[2];
				out[o + 3] = 255;
				continue;
			}
			// 違わない画素は a と同じ位置の画素（両方に存在する）を、白へ寄せた灰色にする
			const s = (y * a.width + x) * 4;
			const gray = 0.3 * a.data[s] + 0.59 * a.data[s + 1] + 0.11 * a.data[s + 2];
			const faded = 255 - (255 - gray) * 0.3;
			out[o] = faded;
			out[o + 1] = faded;
			out[o + 2] = faded;
			out[o + 3] = 255;
		}
	}
	return out;
}

/** `#rrggbb` の色をチャンネルに分ける。形が合わなければ null（CSS の色の値をキャンバス用に読むため） */
export function parseHexColor(value: string): [number, number, number] | null {
	const match = /^#([0-9a-fA-F]{6})$/.exec(value.trim());
	if (!match) return null;
	const n = Number.parseInt(match[1], 16);
	return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function maxChannelDelta(a: Pixels, b: Pixels, x: number, y: number): number {
	const sa = (y * a.width + x) * 4;
	const sb = (y * b.width + x) * 4;
	let max = 0;
	for (let c = 0; c < 4; c++) {
		const d = Math.abs(a.data[sa + c] - b.data[sb + c]);
		if (d > max) max = d;
	}
	return max;
}

function assertShape(p: Pixels, name: string): void {
	if (p.width < 0 || p.height < 0 || p.data.length !== p.width * p.height * 4) {
		throw new RangeError(`pixel data of ${name} does not match its size`);
	}
}
