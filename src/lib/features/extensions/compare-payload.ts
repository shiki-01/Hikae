// 比較の表示へ渡す内容の組み立て（純関数）。差分の結果（FileDiff・OfficeDiff・FilePreview）と、画像の中身を、
// `CompareRenderMessage` の形にする。DOM に依存しない（設計書 11.7）。
// 差分の計算はホスト（Rust）が行う。ここでは、その結果を表示の対象の形に直すだけ。

import type {
	FileDiff,
	FilePreview,
	OfficeDiff,
	PreviewUnavailableReason
} from '#lib/api/types.js';
import { countBlocks } from '#lib/components/diff-rows.js';
import type { DiffStatus, PreviewKind } from '#lib/features/compare/preview-kind.js';
import { mediaMimeOf } from './render-content.js';
import { extOf, fileNameOf } from './renderer-select.js';
import {
	RENDERER_PROTOCOL_VERSION,
	type CompareOptions,
	type CompareRenderMessage,
	type CompareSide,
	type DiffPayload,
	type FileInfo,
	type RenderContent,
	type RenderFile
} from './renderer-protocol.js';

/** 非同期の取得の状態（クエリの結果のうち、組み立てに使う部分だけ） */
export interface Settled<T> {
	pending: boolean;
	failed: boolean;
	data: T | undefined;
}

/** 画像の中身の取得の結果（app-blob）。`absent` は 404（その時点にファイルが無い） */
export type MediaFetch =
	{ status: 'ok'; data: ArrayBuffer } | { status: 'absent' } | { status: 'error' };

/** 片側の内容の状態（ホストが決める）。`failed` は画像の読み取りの失敗（拡張の側で「読めない」と描く） */
export type SideResult =
	| { state: 'ok'; content?: RenderContent }
	| { state: 'missing' }
	| { state: 'unavailable'; reason: PreviewUnavailableReason | 'failed' };

/** 内容の読み取り（preview）の結果から決めた片側の状態。読み取りの失敗は含まない */
export type PreviewSide =
	| { state: 'ok'; content: RenderContent }
	| { state: 'missing' }
	| { state: 'unavailable'; reason: PreviewUnavailableReason };

/** 組み立ての入力（クエリの結果）。使わない種類の入力は、どんな値でもよい */
export interface CompareInput {
	kind: PreviewKind;
	/** ファイルのパス（名前と拡張子だけを使う） */
	path: string;
	/** 過去・今の時点の表示名 */
	fromLabel: string;
	toLabel: string;
	/** 画像の中身（過去・今） */
	media: { from: Settled<MediaFetch>; to: Settled<MediaFetch> };
	/** テキストの行差分（kind が other のとき） */
	text: Settled<FileDiff>;
	/** Office 文書の比較（docx・xlsx） */
	office: Settled<OfficeDiff>;
	/** Office 文書の比較ができなかったときの、ファイルの情報（docx のみ使う） */
	info: Settled<FileDiff>;
	/** xlsx の過去・今の内容 */
	from: Settled<FilePreview>;
	to: Settled<FilePreview>;
}

/** 組み立ての結果が決まったとき（表示の対象が揃った） */
export interface ReadyModel {
	state: 'ready';
	left: CompareSide;
	right: CompareSide;
	diff?: DiffPayload;
}

/** 組み立ての結果。取得の途中か、取得に失敗したか、表示の対象が揃ったか */
export type CompareModel = { state: 'pending' } | { state: 'failed' } | ReadyModel;

/** 表示対象のファイル（名前・拡張子・状態・理由）。パスは含めない */
export function fileOf(path: string, state: RenderFile['state'], reason?: string): RenderFile {
	const file: RenderFile = { name: fileNameOf(path), ext: extOf(path), state };
	if (reason !== undefined) file.reason = reason;
	return file;
}

/** 片側の状態を、比較の 1 側の形にする */
export function sideOf(path: string, label: string, result: SideResult): CompareSide {
	switch (result.state) {
		case 'ok':
			return { label, file: fileOf(path, 'ok'), content: result.content };
		case 'missing':
			return { label, file: fileOf(path, 'missing') };
		case 'unavailable':
			return { label, file: fileOf(path, 'unavailable', result.reason) };
	}
}

/** 画像の中身の取得の結果を、片側の状態にする（mime は拡張子から決める） */
export function sideOfMedia(ext: string, fetched: MediaFetch): SideResult {
	switch (fetched.status) {
		case 'ok':
			return {
				state: 'ok',
				content: {
					kind: 'bytes',
					mime: mediaMimeOf(ext) ?? 'application/octet-stream',
					data: fetched.data
				}
			};
		case 'absent':
			return { state: 'missing' };
		case 'error':
			return { state: 'unavailable', reason: 'failed' };
	}
}

/** 内容の読み取り（preview）の結果を、片側の状態にする。xlsx のシートだけを内容として使う */
export function sideOfPreview(preview: FilePreview): PreviewSide {
	switch (preview.kind) {
		case 'xlsx':
			return { state: 'ok', content: { kind: 'cells', sheets: preview.sheets } };
		case 'not_found':
			return { state: 'missing' };
		case 'unavailable':
			return { state: 'unavailable', reason: preview.reason };
		default:
			return { state: 'unavailable', reason: 'unsupported_format' };
	}
}

/** 比較のファイル情報（内容を比べられなかったときに出す）。テキストの結果が情報のときだけ */
export function infoOf(diff: FileDiff): FileInfo | undefined {
	if (diff.kind !== 'info') return undefined;
	return {
		fileKind: diff.fileKind,
		sizeBytes: diff.sizeBytes,
		modifiedAt: diff.modifiedAt?.getTime() ?? null
	};
}

/** テキストの行差分の結果を、表示の差分の形にする */
export function payloadOfText(diff: FileDiff): DiffPayload {
	switch (diff.kind) {
		case 'text':
			return { kind: 'lines', rows: diff.rows };
		case 'identical':
			return { kind: 'identical' };
		case 'new_empty':
			return { kind: 'new_empty' };
		case 'too_large':
			return { kind: 'too_large', sizeBytes: diff.sizeBytes };
		case 'info':
			// 内容を比べられない（バイナリ）。ファイルの情報を添える
			return { kind: 'unavailable', reason: 'binary', info: infoOf(diff) };
	}
}

/** Office 文書の比較の結果を、表示の差分の形にする。`info` は比べられなかったときのファイル情報 */
export function payloadOfOffice(diff: OfficeDiff, info: FileInfo | undefined): DiffPayload {
	switch (diff.kind) {
		case 'docx':
			return { kind: 'paragraphs', rows: diff.rows, truncated: diff.truncated };
		case 'xlsx':
			return {
				kind: 'cells',
				addedSheets: diff.addedSheets,
				removedSheets: diff.removedSheets,
				cells: diff.cells,
				truncated: diff.truncated
			};
		case 'unavailable':
			return { kind: 'unavailable', reason: diff.reason, info };
	}
}

function pendingIf(...settled: Settled<unknown>[]): boolean {
	return settled.some((s) => s.pending);
}

function failedIf(...settled: Settled<unknown>[]): boolean {
	return settled.some((s) => s.failed);
}

/** 画像の比較。両側の中身が揃ったら表示の対象にする（差分は無い。画素の比べ方は表示の側で行う） */
function imageModel(input: CompareInput): CompareModel {
	const { media } = input;
	if (pendingIf(media.from, media.to)) return { state: 'pending' };
	const ext = extOf(input.path);
	const from =
		media.from.failed || !media.from.data ? { status: 'error' as const } : media.from.data;
	const to = media.to.failed || !media.to.data ? { status: 'error' as const } : media.to.data;
	return {
		state: 'ready',
		left: sideOf(input.path, input.fromLabel, sideOfMedia(ext, from)),
		right: sideOf(input.path, input.toLabel, sideOfMedia(ext, to))
	};
}

/** テキストの行差分。両側は内容を使わない（行の差分に含まれる） */
function textModel(input: CompareInput): CompareModel {
	const { text } = input;
	if (text.pending) return { state: 'pending' };
	if (text.failed || !text.data) return { state: 'failed' };
	return bothOk(input, payloadOfText(text.data));
}

/** docx の段落差分。比べられないときは、ファイルの情報（テキストの比較の結果）を添える */
function docxModel(input: CompareInput): CompareModel {
	const { office, info } = input;
	if (office.pending) return { state: 'pending' };
	if (office.failed || !office.data) return { state: 'failed' };
	if (office.data.kind !== 'unavailable')
		return bothOk(input, payloadOfOffice(office.data, undefined));
	// 情報の取得は、比べられなかったときだけ行う
	if (info.pending) return { state: 'pending' };
	const fileInfo = !info.failed && info.data ? infoOf(info.data) : undefined;
	return bothOk(input, payloadOfOffice(office.data, fileInfo));
}

/** xlsx のシートとセルの差分。片側が無い・読めないときは、その理由を表示する */
function xlsxModel(input: CompareInput): CompareModel {
	const { office, from, to } = input;
	if (pendingIf(office, from, to)) return { state: 'pending' };
	if (failedIf(office, from, to) || !office.data || !from.data || !to.data) {
		return { state: 'failed' };
	}
	const fromSide = sideOfPreview(from.data);
	const toSide = sideOfPreview(to.data);
	const reason: PreviewUnavailableReason | null =
		toSide.state === 'unavailable'
			? toSide.reason
			: fromSide.state === 'unavailable'
				? fromSide.reason
				: office.data.kind === 'unavailable'
					? office.data.reason
					: null;
	const left = sideOf(input.path, input.fromLabel, fromSide);
	const right = sideOf(input.path, input.toLabel, toSide);
	if (reason !== null) {
		return { state: 'ready', left, right, diff: { kind: 'unavailable', reason } };
	}
	if (office.data.kind !== 'xlsx') {
		return {
			state: 'ready',
			left,
			right,
			diff: { kind: 'unavailable', reason: 'unsupported_format' }
		};
	}
	return { state: 'ready', left, right, diff: payloadOfOffice(office.data, undefined) };
}

/** 種類ごとに組み立てる（pdf・pptx は比較の表示を使わないため、failed を返す） */
export function compareModelOf(input: CompareInput): CompareModel {
	switch (input.kind) {
		case 'image':
			return imageModel(input);
		case 'other':
			return textModel(input);
		case 'docx':
			return docxModel(input);
		case 'xlsx':
			return xlsxModel(input);
		default:
			return { state: 'failed' };
	}
}

/** 両側とも内容を持つ（テキスト・docx の表示の既定の形） */
function bothOk(input: CompareInput, diff: DiffPayload): CompareModel {
	return {
		state: 'ready',
		left: sideOf(input.path, input.fromLabel, { state: 'ok' }),
		right: sideOf(input.path, input.toLabel, { state: 'ok' }),
		diff
	};
}

/**
 * 比較の表示の 1 回分のメッセージを作る。画像の中身は複製して転送する（キャッシュの元は残す）。
 * `transfer` は postMessage の転送リスト
 */
export function buildCompareMessage(
	id: number,
	model: ReadyModel,
	options: CompareOptions
): { message: CompareRenderMessage; transfer: ArrayBuffer[] } {
	const transfer: ArrayBuffer[] = [];
	const copySide = (side: CompareSide): CompareSide => {
		const content = side.content;
		if (content?.kind !== 'bytes') return side;
		const copy = content.data.slice(0);
		transfer.push(copy);
		return { ...side, content: { ...content, data: copy } };
	};
	const message: CompareRenderMessage = {
		v: RENDERER_PROTOCOL_VERSION,
		type: 'render',
		id,
		mode: 'compare',
		left: copySide(model.left),
		right: copySide(model.right),
		options
	};
	if (model.diff !== undefined) message.diff = model.diff;
	return { message, transfer };
}

/** 比較の本体の状態（親の前へ・次へ、表示方法の表示に使う）。行差分のときだけ箇所の数を持つ */
export function diffStatusOf(model: CompareModel): DiffStatus {
	if (model.state === 'pending') return { textual: false, ready: false, total: 0 };
	if (model.state === 'failed') return { textual: false, ready: true, total: 0 };
	const diff = model.diff;
	const textual =
		diff !== undefined &&
		(diff.kind === 'lines' ||
			diff.kind === 'identical' ||
			diff.kind === 'new_empty' ||
			diff.kind === 'too_large');
	const total = diff?.kind === 'lines' ? countBlocks(diff.rows) : 0;
	return { textual, ready: true, total };
}
