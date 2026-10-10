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

/** 変更差分の本体の種類。`pending` は、テキストかどうかの判定の結果がまだ無いこと */
export type DiffBody = 'image' | 'docx' | 'xlsx' | 'side_by_side' | 'text' | 'pending';

/**
 * 変更差分の本体をどれで描くかを決める。
 * pdf・pptx・内容を表示できないその他のファイルは、過去と今を並べて見せる（side_by_side）。
 * その他のファイルは、行差分の結果が「情報のみ」（info）のときだけ side_by_side にし、テキストなら text にする。
 */
export function diffBodyOf(kind: PreviewKind, textDiff: FileDiff | undefined): DiffBody {
	switch (kind) {
		case 'image':
			return 'image';
		case 'docx':
			return 'docx';
		case 'xlsx':
			return 'xlsx';
		case 'pdf':
		case 'pptx':
			return 'side_by_side';
		case 'other':
			if (textDiff === undefined) return 'pending';
			return textDiff.kind === 'info' ? 'side_by_side' : 'text';
	}
}
