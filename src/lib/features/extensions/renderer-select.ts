// 表示拡張の選択と URL（純関数）。拡張子から入口（iframe の HTML）を決め、`ext://` の URL を組み立てる。

import type { RendererInfo } from '#lib/api/types.js';

/** 全部の拡張子を表す宣言（最も低い優先度） */
const FALLBACK_FILE_TYPE = '*';

/** パスの末尾の要素（ファイル名）。区切りは `/` と `\` の両方 */
export function fileNameOf(path: string): string {
	return path.split(/[\\/]/).pop() ?? '';
}

/** 拡張子（`.` を除いた英小文字）。先頭の `.` だけの名前は拡張子なし（Rust 側の Path::extension と同じ） */
export function extOf(path: string): string {
	const name = fileNameOf(path);
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return '';
	return name.slice(dot + 1).toLowerCase();
}

/**
 * 拡張子と方法から、使う表示を決める。ホスト（Rust の `renderer_for`）と同じ規則:
 * その拡張子を宣言した表示が 1 つでもあれば、その中から方法に対応するものを選ぶ（無ければ null。`*` には落とさない）。
 * 宣言の無い拡張子だけ、`*` の宣言から選ぶ
 */
function pick(
	renderers: readonly RendererInfo[],
	ext: string,
	mode: 'view' | 'compare'
): RendererInfo | null {
	const own = renderers.filter((r) => r.fileTypes.includes(ext));
	const pool =
		own.length > 0 ? own : renderers.filter((r) => r.fileTypes.includes(FALLBACK_FILE_TYPE));
	return pool.find((r) => r.modes.includes(mode)) ?? null;
}

/** 表示（view）に使う表示を決める */
export function rendererFor(renderers: readonly RendererInfo[], ext: string): RendererInfo | null {
	return pick(renderers, ext, 'view');
}

/**
 * 比較（compare）に使う表示を決める。pdf・pptx のように compare を宣言しない形式は null で、
 * ホストが view を 2 つ並べる（設計書 11.2）
 */
export function compareRendererFor(
	renderers: readonly RendererInfo[],
	ext: string
): RendererInfo | null {
	return pick(renderers, ext, 'compare');
}

/** `ext://` の URL（Windows の WebView は `http://ext.localhost/` に変換して読む）。`entryUrlPath` はセグメントごとに扱う */
export function extUrl(entryUrlPath: string, windows: boolean): string {
	const base = windows ? 'http://ext.localhost' : 'ext://localhost';
	return `${base}/${encodeURI(entryUrlPath)}`;
}
