// 表示拡張（iframe）とホストの通信（設計書 11 章 `プロトコル v1`）。DOM に依存しない純関数と型だけを置く。
//
// - ホスト → iframe: `render`（表示の対象）。データは postMessage のみで渡し、パス・トークン・設定は送らない
// - iframe → ホスト: `ready`（読み込み完了。1 回）、`error`（表示できなかった）、`open`（「ファイルを開く」の要求）
// - 受け取る側は、どちらも `v === 1` と `type` が既知のものだけを受理し、それ以外は無視する
// - iframe は opaque origin（sandbox="allow-scripts"）のため、origin は "null" になる。送信先は "*" で、
//   受信側は `event.source` で相手を確かめる

import type { PreviewSheet, PreviewSlide } from '#lib/api/types.js';

/** プロトコルの版。変えるときは両側を同時に直す */
export const RENDERER_PROTOCOL_VERSION = 1 as const;

/** iframe の読み込みを待つ時間（ms）。超えたら拡張の失敗として通常の表示へ切り替える */
export const RENDERER_READY_TIMEOUT_MS = 5000;

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

/** ホスト → iframe: 表示の対象。`id` は表示ごとの番号（失敗の報告と突き合わせる） */
export interface RenderMessage {
	v: typeof RENDERER_PROTOCOL_VERSION;
	type: 'render';
	id: number;
	mode: 'view';
	file: RenderFile;
	/** `file.state` が `ok` のときだけ入る */
	content?: RenderContent;
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
			if (typeof id !== 'number' || !Number.isSafeInteger(id) || id < 0) return null;
			if (code !== 'render-failed' && code !== 'unsupported') return null;
			return { v: RENDERER_PROTOCOL_VERSION, type: 'error', id, code };
		}
		default:
			return null;
	}
}

/**
 * ホストから届いた `render` を検査する（iframe 側）。形が合わなければ null。
 * 内容の種類の細かい検査は、表示の側で行う（壊れていれば `render-failed` になる）
 */
export function parseRenderMessage(data: unknown): RenderMessage | null {
	if (typeof data !== 'object' || data === null) return null;
	const record = data as Record<string, unknown>;
	if (record.v !== RENDERER_PROTOCOL_VERSION || record.type !== 'render') return null;
	if (typeof record.id !== 'number' || !Number.isSafeInteger(record.id)) return null;
	if (record.mode !== 'view') return null;
	const file = record.file as Partial<RenderFile> | null | undefined;
	if (typeof file !== 'object' || file === null) return null;
	if (typeof file.name !== 'string' || typeof file.ext !== 'string') return null;
	if (file.state !== 'ok' && file.state !== 'missing' && file.state !== 'unavailable') return null;
	return record as unknown as RenderMessage;
}
