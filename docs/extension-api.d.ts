// 表示拡張（renderer）と、ホストとの通信プロトコル v1 の型（設計書 11.7）。
//
// これは仕様の書き出しであり、パッケージではない（型定義パッケージは未実装。設計書 11.3）。
// 表示拡張の作者は、この内容を参照して ui/ の受け取りを書く。実装の正は次のとおり:
//   - 送る内容: src/lib/features/extensions/renderer-protocol.ts（ホスト側の検査と型）
//   - 描く内容: src/lib/ext-ui/（組み込みの表示拡張の描画部品）
// 食い違いがあれば、設計書 11.7 とこのファイルを直す。
//
// 通信（すべて postMessage。送信先は "*"、受け取り側は event.source が親（ホスト）の窓か確かめる）:
//   - iframe の属性: sandbox="allow-scripts"（allow-same-origin は付けない）。CSP は connect-src 'none'
//   - 1 回だけ ready を送る → ホストが render を送る → 失敗は error で返す
//   - 版（v）が 1 以外、未知の type、型の違うメッセージは、双方とも無視する

/** プロトコルの版 */
export type ProtocolVersion = 1;

/** 表示の方法。`compare` は比較（2 時点）で使う */
export type RenderMode = 'view' | 'compare';

/** 表示対象の状態。missing は、その時点にファイルが無いこと。unavailable は、内容を表示できないこと（理由つき） */
export type RenderFileState = 'ok' | 'missing' | 'unavailable';

/** 表示対象のファイル（名前と拡張子だけ。パス・プロジェクト・時点の ID は送らない） */
export interface RenderFile {
	/** ファイル名（末尾の要素のみ） */
	name: string;
	/** 拡張子（`.` を除いた英小文字。無ければ空文字） */
	ext: string;
	state: RenderFileState;
	/** unavailable のときの理由（読み取れなかったときは `failed`） */
	reason?: string;
}

/** xlsx のセル 1 件（行・列は 0 始まり） */
export interface PreviewCell {
	row: number;
	col: number;
	value: string;
}

/** xlsx のシート 1 枚。`cells` は値のあるセルだけ */
export interface PreviewSheet {
	name: string;
	/** A1 から最後の値までの行数 */
	rows: number;
	/** A1 から最後の値までの列数 */
	cols: number;
	cells: PreviewCell[];
	/** `cells` に入らず打ち切ったか */
	truncated: boolean;
}

/** pptx のスライド 1 枚（`index` は 1 始まり） */
export interface PreviewSlide {
	index: number;
	paragraphs: string[];
}

/** 表示の内容（種類ごと）。`bytes` の `data` は Transferable として渡される */
export type RenderContent =
	| { kind: 'bytes'; mime: string; data: ArrayBuffer }
	| { kind: 'text'; text: string; truncated: boolean }
	| { kind: 'paragraphs'; paragraphs: string[]; truncated: boolean }
	| { kind: 'cells'; sheets: PreviewSheet[] }
	| { kind: 'pages'; pages: PreviewSlide[]; truncated: boolean };

/** テキストの行差分の 1 行 */
export interface DiffRow {
	kind: 'context' | 'add' | 'del';
	oldNo: number | null;
	newNo: number | null;
	text: string;
}

/** docx の段落差分の 1 行（`omitted` は省略された変更のない段落の件数） */
export type ParagraphRow =
	| { kind: 'context'; oldNo: number; newNo: number; text: string }
	| { kind: 'add'; newNo: number; text: string }
	| { kind: 'del'; oldNo: number; text: string }
	| { kind: 'omitted'; count: number };

/** xlsx のセルの変更 1 件。`before` が null なら追加、`after` が null なら削除 */
export interface CellChange {
	sheet: string;
	/** セル参照（A1 形式） */
	cell: string;
	row: number;
	col: number;
	before: string | null;
	after: string | null;
}

/** 比較の差分（種類ごと）。ホストが計算する。拡張は計算しない */
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
	| {
			kind: 'unavailable';
			reason: 'too_large' | 'unsupported_format' | 'corrupt' | 'binary';
			info?: { fileKind: string; sizeBytes: number; modifiedAt: number | null };
	  };

/** 比較の 1 側（左が過去、右が今） */
export interface CompareSide {
	/** 時点の表示名（見出し） */
	label: string;
	file: RenderFile;
	/** 画像（bytes）と xlsx（cells）の側だけ */
	content?: RenderContent;
}

/** 比較の表示の選択。表示は作り直さず、この値だけを変える */
export interface CompareOptions {
	/** 行差分の表示（並べる・重ねる） */
	viewMode: 'split' | 'unified';
	/** 強調する箇所の番号（無ければ null） */
	activeBlock: number | null;
}

/** ホスト → iframe: 表示（1 時点）。`id` は表示ごとの番号（error と突き合わせる） */
export interface ViewRenderMessage {
	v: ProtocolVersion;
	type: 'render';
	id: number;
	mode: 'view';
	file: RenderFile;
	/** `file.state` が `ok` のときだけ入る */
	content?: RenderContent;
}

/** ホスト → iframe: 比較（2 時点）。`diff` が無いときは画像の比較（左右の `content` を使う） */
export interface CompareRenderMessage {
	v: ProtocolVersion;
	type: 'render';
	id: number;
	mode: 'compare';
	left: CompareSide;
	right: CompareSide;
	diff?: DiffPayload;
	options: CompareOptions;
}

/** ホスト → iframe: 比較の表示の切り替え（同じ `id` の表示に適用する） */
export interface RenderOptionsMessage {
	v: ProtocolVersion;
	type: 'options';
	id: number;
	options: CompareOptions;
}

/** ホスト → iframe の全メッセージ */
export type HostToRendererMessage = ViewRenderMessage | CompareRenderMessage | RenderOptionsMessage;

/** iframe → ホスト: 読み込みの完了（1 回だけ） */
export interface ReadyEvent {
	v: ProtocolVersion;
	type: 'ready';
}

/** iframe → ホスト: 表示の失敗。`render-failed` は描けなかった、`unsupported` は内容の種類を扱えない */
export interface RendererErrorEvent {
	v: ProtocolVersion;
	type: 'error';
	id: number;
	code: 'render-failed' | 'unsupported';
}

/** iframe → ホスト: 「ファイルを開く」の要求（ホストは既定のアプリで開く） */
export interface OpenEvent {
	v: ProtocolVersion;
	type: 'open';
}

/** iframe → ホストの全メッセージ */
export type RendererToHostMessage = ReadyEvent | RendererErrorEvent | OpenEvent;

/** マニフェスト（extension.json）の表示の宣言（設計書 11.2） */
export interface RendererDeclaration {
	/** `.` で始まる英小文字と数字の拡張子の並び、または `["*"]`（全部の拡張子。最も低い優先度） */
	fileTypes: string[];
	/** `view`・`compare` の空でない並び */
	modes: RenderMode[];
	/** 拡張フォルダ内の相対パス（例: `ui/index.html`） */
	entry: string;
}
