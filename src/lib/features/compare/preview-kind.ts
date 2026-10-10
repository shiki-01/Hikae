// ファイルの種類による表示の振り分け（設計書 デザイン要件 S2・S4、ChangeDetail・CompareView 共通）。DOM に依存しない純関数。

import type { FileDiff } from '#lib/api/types.js';
import { isImagePath } from './image-diff';
import { officeKindOf } from './office-diff';

/** 表示の振り分けに使う種類。other はテキストかバイナリかを、内容の読み取り結果で決める */
export type PreviewKind = 'image' | 'pdf' | 'docx' | 'xlsx' | 'pptx' | 'other';

/** パスの拡張子から種類を決める（大文字小文字は区別しない） */
export function previewKindOf(path: string): PreviewKind {
	if (isImagePath(path)) return 'image';
	const name = path.split(/[\\/]/).pop() ?? '';
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return 'other';
	const ext = name.slice(dot + 1).toLowerCase();
	if (ext === 'pdf') return 'pdf';
	if (ext === 'pptx') return 'pptx';
	return officeKindOf(path) ?? 'other';
}

/** 変更差分の本体の状態（親の表示に使う） */
export interface DiffStatus {
	/** 行差分か（前へ・次へ、表示方法を出すかどうか） */
	textual: boolean;
	/** 本体の種類が決まったか */
	ready: boolean;
	/** 行差分の箇所の数（行差分のときだけ意味がある） */
	total: number;
}

/** 比較の表示の振り分け。frame は拡張（compare）で描く、side は過去と今を並べる、pending は判定の途中 */
export type CompareRoute = 'pending' | 'frame' | 'side';

export interface CompareRouteInput {
	kind: PreviewKind;
	/** 表示の一覧の状態（起動中は 1 回取る） */
	renderers: 'pending' | 'failed' | 'ready';
	/** 比較に使える表示があるか（compareRendererFor の結果） */
	hasCompareRenderer: boolean;
	/** テキストの行差分の状態（kind が other のときだけ使う）。info はテキストとして比べられないもの */
	text: { pending: boolean; diff: FileDiff | undefined };
}

/**
 * 比較の表示を決める（設計書 11.2）。
 * - pdf・pptx は compare を宣言しないため、過去と今を並べる（side）
 * - compare に使える表示が無い・一覧の取得に失敗したときも side
 * - その他のファイルは、行差分の結果が情報のみ（info）なら side、テキストなら拡張で描く（frame）
 */
export function compareRouteOf(input: CompareRouteInput): CompareRoute {
	if (input.kind === 'pdf' || input.kind === 'pptx') return 'side';
	if (input.renderers === 'pending') return 'pending';
	if (input.renderers === 'failed' || !input.hasCompareRenderer) return 'side';
	if (input.kind === 'other') {
		if (input.text.pending) return 'pending';
		if (input.text.diff?.kind === 'info') return 'side';
	}
	return 'frame';
}
