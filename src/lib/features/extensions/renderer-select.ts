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
 * 拡張子と方法から、使う表示を決める。同じ拡張子の宣言が優先し、無ければ `*`。
 * ホスト（`renderer_for`）と同じ規則。`view` の宣言だけを対象にする
 */
export function rendererFor(
	renderers: readonly RendererInfo[],
	ext: string,
	mode: 'view' = 'view'
): RendererInfo | null {
	const find = (wanted: string) =>
		renderers.find((r) => r.modes.includes(mode) && r.fileTypes.includes(wanted)) ?? null;
	return find(ext) ?? find(FALLBACK_FILE_TYPE);
}

/**
 * 比較（compare）に使う表示を決める。拡張子の宣言が 1 つでもあれば、その中から compare を選ぶ（無ければ null）。
 * `*` には落とさない。pdf・pptx のように compare を宣言しない形式は、ホストが view を 2 つ並べる（設計書 11.2）。
 * 宣言の無い拡張子だけ、`*` の compare を使う
 */
export function compareRendererFor(
	renderers: readonly RendererInfo[],
	ext: string
): RendererInfo | null {
	const own = renderers.filter((r) => r.fileTypes.includes(ext));
	if (own.length > 0) return own.find((r) => r.modes.includes('compare')) ?? null;
	return (
		renderers.find(
			(r) => r.fileTypes.includes(FALLBACK_FILE_TYPE) && r.modes.includes('compare')
		) ?? null
	);
}

/** `ext://` の URL（Windows の WebView は `http://ext.localhost/` に変換して読む）。`entryUrlPath` はセグメントごとに扱う */
export function extUrl(entryUrlPath: string, windows: boolean): string {
	const base = windows ? 'http://ext.localhost' : 'ext://localhost';
	return `${base}/${encodeURI(entryUrlPath)}`;
}
