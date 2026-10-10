// 表示拡張へ渡す内容の組み立て（純関数）。`FilePreview`（読み取りの結果）と、画像・pdf の中身を、`RenderMessage` の形にする。

import type { FilePreview } from '#lib/api/types.js';
import { extOf, fileNameOf } from './renderer-select';
import type { RenderContent, RenderMessage } from './renderer-protocol';
import { RENDERER_PROTOCOL_VERSION } from './renderer-protocol';

/** 表示の準備の結果（`bytes` は画像・pdf の中身だけ） */
export type RenderOutcome =
	| { state: 'ok'; content: RenderContent }
	| { state: 'missing' }
	| { state: 'unavailable'; reason: string };

/** 画像・pdf の拡張子 → mime（表示拡張が画像として描く・pdf として読むために使う） */
const MEDIA_MIME: Record<string, string> = {
	png: 'image/png',
	jpg: 'image/jpeg',
	jpeg: 'image/jpeg',
	gif: 'image/gif',
	webp: 'image/webp',
	bmp: 'image/bmp',
	pdf: 'application/pdf'
};

/** 拡張子から mime を決める（画像・pdf 以外は null） */
export function mediaMimeOf(ext: string): string | null {
	return MEDIA_MIME[ext] ?? null;
}

/** 読み取りの結果を表示の内容にする（画像・pdf の `binary` は呼び出し側が中身を読んで別に扱う） */
export function outcomeOfPreview(preview: FilePreview): RenderOutcome {
	switch (preview.kind) {
		case 'text':
			return {
				state: 'ok',
				content: { kind: 'text', text: preview.content, truncated: preview.truncated }
			};
		case 'docx':
			return {
				state: 'ok',
				content: {
					kind: 'paragraphs',
					paragraphs: preview.paragraphs,
					truncated: preview.truncated
				}
			};
		case 'pptx':
			return {
				state: 'ok',
				content: { kind: 'pages', pages: preview.slides, truncated: preview.truncated }
			};
		case 'xlsx':
			return { state: 'ok', content: { kind: 'cells', sheets: preview.sheets } };
		case 'unavailable':
			return { state: 'unavailable', reason: preview.reason };
		case 'not_found':
			return { state: 'missing' };
	}
}

/**
 * 表示の 1 回分のメッセージを作る。`bytes` は複製して渡す（元のバッファは呼び出し側のキャッシュに残すため）。
 * 戻り値の `transfer` は postMessage の転送リスト
 */
export function buildRenderMessage(
	id: number,
	path: string,
	outcome: RenderOutcome
): { message: RenderMessage; transfer: ArrayBuffer[] } {
	const name = fileNameOf(path);
	const ext = extOf(path);
	const transfer: ArrayBuffer[] = [];
	let message: RenderMessage;
	if (outcome.state === 'ok') {
		let content = outcome.content;
		if (content.kind === 'bytes') {
			const copy = content.data.slice(0);
			transfer.push(copy);
			content = { kind: 'bytes', mime: content.mime, data: copy };
		}
		message = {
			v: RENDERER_PROTOCOL_VERSION,
			type: 'render',
			id,
			mode: 'view',
			file: { name, ext, state: 'ok' },
			content
		};
	} else if (outcome.state === 'missing') {
		message = {
			v: RENDERER_PROTOCOL_VERSION,
			type: 'render',
			id,
			mode: 'view',
			file: { name, ext, state: 'missing' }
		};
	} else {
		message = {
			v: RENDERER_PROTOCOL_VERSION,
			type: 'render',
			id,
			mode: 'view',
			file: { name, ext, state: 'unavailable', reason: outcome.reason }
		};
	}
	return { message, transfer };
}
