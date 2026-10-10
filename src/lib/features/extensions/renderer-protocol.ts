// 表示拡張（iframe）とホストの通信（設計書 11 章 `プロトコル v1`）。DOM に依存しない純関数と型だけを置く。
//
// - ホスト → iframe: `render`（表示の対象。view または compare）、`options`（比較の表示の切り替え）
// - iframe → ホスト: `ready`（読み込み完了。1 回）、`error`（表示できなかった）、`open`（「ファイルを開く」の要求）
// - 受け取る側は、どちらも `v === 1` と `type` が既知のものだけを受理し、それ以外は無視する
// - iframe は opaque origin（sandbox="allow-scripts"）のため、origin は "null" になる。送信先は "*" で、
//   受信側は `event.source` で相手を確かめる
// - 比較の差分（行・段落・セル）の計算はホストが行い、iframe は受け取った結果を描くだけ

import type {
	CellChange,
	DiffRow,
	FileKind,
	ParagraphRow,
	PreviewSheet,
	PreviewSlide,
	PreviewUnavailableReason
} from '#lib/api/types.js';

/** プロトコルの版。変えるときは両側を同時に直す（比較の追加は v のまま、新しい種類のメッセージとして足した） */
export const RENDERER_PROTOCOL_VERSION = 1 as const;

/** iframe の読み込みを待つ時間（ms）。超えたら拡張の失敗として通常の表示へ切り替える */
export const RENDERER_READY_TIMEOUT_MS = 5000;

/** 表示の方法（設計書 11.2 の `modes`） */
export type RenderMode = 'view' | 'compare';

/** 表示対象の状態。missing はその時点にファイルが無いこと、unavailable は内容を表示できないこと（理由つき） */
export type RenderFileState = 'ok' | 'missing' | 'unavailable';

/** 表示対象のファイル（名前と拡張子だけ。パスは送らない） */
export interface RenderFile {
	/** ファイル名（末尾の要素のみ） */
	name: string;
	/** 拡張子（`.` を除いた英小文字。無ければ空文字） */
	ext: string;
	state: RenderFileState;
	/** unavailable のときの理由（`PreviewUnavailableReason` の語。読み取れなかったときは `failed`） */
	reason?: string;
}

/** 表示の内容（種類ごと）。`bytes` の `data` は Transferable として渡す */
export type RenderContent =
	| { kind: 'bytes'; mime: string; data: ArrayBuffer }
	| { kind: 'text'; text: string; truncated: boolean }
	| { kind: 'paragraphs'; paragraphs: string[]; truncated: boolean }
	| { kind: 'cells'; sheets: PreviewSheet[] }
	| { kind: 'pages'; pages: PreviewSlide[]; truncated: boolean };

/** ファイルの情報（内容を表示できないときに出す。更新日時はミリ秒、無ければ null） */
export interface FileInfo {
	fileKind: FileKind;
	sizeBytes: number;
	modifiedAt: number | null;
}

/**
 * 比較の差分の結果（種類ごと）。ホストが計算する。
 * - lines: テキストの行差分（`DiffRow` の並び）
 * - paragraphs: docx の段落差分（省略された行を含む）
 * - cells: xlsx のセルの変更と、シートの追加・削除
 * - identical・new_empty・too_large: テキストの比較の注記
 * - unavailable: 比べられなかった（理由と、取れたときはファイルの情報）
 */
export type DiffPayload =
	| { kind: 'lines'; rows: DiffRow[] }
	| { kind: 'paragraphs'; rows: ParagraphRow[]; truncated: number }
	| {
			kind: 'cells';
			addedSheets: string[];
			removedSheets: string[];
			cells: CellChange[];
			truncated: number;
	  }
	| { kind: 'identical' }
	| { kind: 'new_empty' }
	| { kind: 'too_large'; sizeBytes: number }
	| { kind: 'unavailable'; reason: PreviewUnavailableReason; info?: FileInfo };

/** 比較の 1 側（左が過去、右が今）。`content` は画像（bytes）と xlsx（cells）の側だけ */
export interface CompareSide {
	/** 時点の表示名（支援技術向けの領域の名前・見出し） */
	label: string;
	file: RenderFile;
	content?: RenderContent;
}

/** 比較の表示の選択。`viewMode` は行差分の表示（並べる・重ねる）、`activeBlock` は強調する箇所の番号 */
export interface CompareOptions {
	viewMode: 'split' | 'unified';
	activeBlock: number | null;
}

/** ホスト → iframe: 表示の対象（1 時点）。`id` は表示ごとの番号（失敗の報告と突き合わせる） */
export interface ViewRenderMessage {
	v: typeof RENDERER_PROTOCOL_VERSION;
	type: 'render';
	id: number;
	mode: 'view';
	file: RenderFile;
	/** `file.state` が `ok` のときだけ入る */
	content?: RenderContent;
}

/** ホスト → iframe: 比較の対象（2 時点）。`diff` が無いときは画像の比較（左右の `content` を使う） */
export interface CompareRenderMessage {
	v: typeof RENDERER_PROTOCOL_VERSION;
	type: 'render';
	id: number;
	mode: 'compare';
	left: CompareSide;
	right: CompareSide;
	diff?: DiffPayload;
	options: CompareOptions;
}

/** ホスト → iframe: 表示の対象 */
export type RenderMessage = ViewRenderMessage | CompareRenderMessage;

/** ホスト → iframe: 比較の表示の切り替え（表示は作り直さず、同じ `id` の表示に適用する） */
export interface RenderOptionsMessage {
	v: typeof RENDERER_PROTOCOL_VERSION;
	type: 'options';
	id: number;
	options: CompareOptions;
}

/** iframe → ホスト */
export type RendererEvent =
	| { v: typeof RENDERER_PROTOCOL_VERSION; type: 'ready' }
	| {
			v: typeof RENDERER_PROTOCOL_VERSION;
			type: 'error';
			id: number;
			code: 'render-failed' | 'unsupported';
	  }
	| { v: typeof RENDERER_PROTOCOL_VERSION; type: 'open' };

/** 表示の番号か（0 以上の安全な整数） */
function isId(value: unknown): value is number {
	return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

/**
 * iframe から届いたメッセージを検査する。`v` が 1 で、`type` が既知で、フィールドの型が合うものだけ返す。
 * それ以外（別の版、未知の種類、型の違い、余分な形）は null
 */
export function parseRendererEvent(data: unknown): RendererEvent | null {
	if (typeof data !== 'object' || data === null) return null;
	const record = data as Record<string, unknown>;
	if (record.v !== RENDERER_PROTOCOL_VERSION) return null;
	switch (record.type) {
		case 'ready':
			return { v: RENDERER_PROTOCOL_VERSION, type: 'ready' };
		case 'open':
			return { v: RENDERER_PROTOCOL_VERSION, type: 'open' };
		case 'error': {
			const { id, code } = record;
			if (!isId(id)) return null;
			if (code !== 'render-failed' && code !== 'unsupported') return null;
			return { v: RENDERER_PROTOCOL_VERSION, type: 'error', id, code };
		}
		default:
			return null;
	}
}

/** 表示対象のファイルの形を検査する（名前・拡張子・状態。理由は文字列のときだけ） */
function parseRenderFile(value: unknown): RenderFile | null {
	if (typeof value !== 'object' || value === null) return null;
	const file = value as Record<string, unknown>;
	if (typeof file.name !== 'string' || typeof file.ext !== 'string') return null;
	if (file.state !== 'ok' && file.state !== 'missing' && file.state !== 'unavailable') return null;
	if (file.reason !== undefined && typeof file.reason !== 'string') return null;
	return file as unknown as RenderFile;
}

/** 比較の表示の選択の形を検査する */
export function parseCompareOptions(value: unknown): CompareOptions | null {
	if (typeof value !== 'object' || value === null) return null;
	const options = value as Record<string, unknown>;
	if (options.viewMode !== 'split' && options.viewMode !== 'unified') return null;
	const block = options.activeBlock;
	if (block !== null && !isId(block)) return null;
	return { viewMode: options.viewMode, activeBlock: block };
}

/** 比較の 1 側の形を検査する（内容の種類の細かい検査は、表示の側で行う） */
function parseCompareSide(value: unknown): CompareSide | null {
	if (typeof value !== 'object' || value === null) return null;
	const side = value as Record<string, unknown>;
	if (typeof side.label !== 'string') return null;
	const file = parseRenderFile(side.file);
	if (!file) return null;
	return { label: side.label, file, content: side.content as RenderContent | undefined };
}

/** 差分の結果の種類を検査する（既知の種類と、必要な形だけ。中身は表示の側で扱う） */
function isDiffPayload(value: unknown): value is DiffPayload {
	if (typeof value !== 'object' || value === null) return false;
	const kind = (value as { kind?: unknown }).kind;
	return (
		kind === 'lines' ||
		kind === 'paragraphs' ||
		kind === 'cells' ||
		kind === 'identical' ||
		kind === 'new_empty' ||
		kind === 'too_large' ||
		kind === 'unavailable'
	);
}

/**
 * ホストから届いた `render` を検査する（iframe 側）。形が合わなければ null。
 * 内容の種類の細かい検査は、表示の側で行う（壊れていれば `render-failed` になる）
 */
export function parseRenderMessage(data: unknown): RenderMessage | null {
	if (typeof data !== 'object' || data === null) return null;
	const record = data as Record<string, unknown>;
	if (record.v !== RENDERER_PROTOCOL_VERSION || record.type !== 'render') return null;
	if (!isId(record.id)) return null;
	if (record.mode === 'view') {
		const file = parseRenderFile(record.file);
		if (!file) return null;
		return { ...record, file } as unknown as ViewRenderMessage;
	}
	if (record.mode === 'compare') {
		const left = parseCompareSide(record.left);
		const right = parseCompareSide(record.right);
		const options = parseCompareOptions(record.options);
		if (!left || !right || !options) return null;
		if (record.diff !== undefined && !isDiffPayload(record.diff)) return null;
		return {
			v: RENDERER_PROTOCOL_VERSION,
			type: 'render',
			id: record.id,
			mode: 'compare',
			left,
			right,
			diff: record.diff as DiffPayload | undefined,
			options
		};
	}
	return null;
}

/** ホストから届いた比較の表示の切り替えを検査する（iframe 側） */
export function parseOptionsMessage(data: unknown): RenderOptionsMessage | null {
	if (typeof data !== 'object' || data === null) return null;
	const record = data as Record<string, unknown>;
	if (record.v !== RENDERER_PROTOCOL_VERSION || record.type !== 'options') return null;
	if (!isId(record.id)) return null;
	const options = parseCompareOptions(record.options);
	if (!options) return null;
	return { v: RENDERER_PROTOCOL_VERSION, type: 'options', id: record.id, options };
}
