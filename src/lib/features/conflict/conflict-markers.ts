// 競合マーカー付きの全文を、共通部分と競合ハンクの列に分ける純関数（設計書 4.4 手順 5）。
// 行は改行コードを保持したまま扱うため、CRLF のファイルでも内容を壊さない。
// 解析できない（マーカーの順序不正、閉じ忘れ、ハンクが無い等）ときは null を返し、画面は行ごとの編集を出さない。

/** 競合マーカーの行頭（改行を除いた行で判定する） */
const OURS = '<<<<<<< ';
const BASE = '||||||| ';
const SEPARATOR = '=======';
const THEIRS = '>>>>>>> ';

/** 競合していない共通部分 */
export interface CommonSegment {
	kind: 'common';
	text: string;
}

/** 競合ハンク。mine はこの PC 側の版、theirs はクラウド側の版（改行つき）。base は diff3 形式の共通の元 */
export interface HunkSegment {
	kind: 'hunk';
	mine: string;
	base: string | null;
	theirs: string;
}

export type ConflictSegment = CommonSegment | HunkSegment;

export interface ConflictDocument {
	segments: ConflictSegment[];
	/** 行末の改行コード。ハンクの間に改行が足りないときの補完に使う */
	eol: '\n' | '\r\n';
}

/** ハンクごとの選択。edited は利用者が編集したテキスト */
export type HunkPick =
	{ kind: 'mine' } | { kind: 'theirs' } | { kind: 'both' } | { kind: 'edited'; text: string };

/** 行を改行つきのまま分ける（最後の行は改行が無いことがある） */
function splitLines(text: string): string[] {
	return text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
}

/** 改行（LF / CRLF）を除いた行。マーカーの判定に使う */
function bare(line: string): string {
	return line.replace(/\r*\n?$/, '');
}

function detectEol(lines: string[]): '\n' | '\r\n' {
	const first = lines.find((line) => line.endsWith('\n'));
	return first !== undefined && first.endsWith('\r\n') ? '\r\n' : '\n';
}

/**
 * 競合マーカー付きの全文を解析する。
 * 解析できない（順序不正・閉じ忘れ・入れ子・ハンクが 1 つも無い）ときは null。
 * `=======` の単独行は、競合の外では通常の文字として扱う。
 */
export function parseConflictMarkers(text: string): ConflictDocument | null {
	const lines = splitLines(text);
	const segments: ConflictSegment[] = [];
	let common = '';
	let state: 'common' | 'mine' | 'base' | 'theirs' = 'common';
	let mine = '';
	let base: string | null = null;
	let theirs = '';

	for (const line of lines) {
		const current = bare(line);
		if (state === 'common') {
			if (current.startsWith(OURS)) {
				if (common !== '') segments.push({ kind: 'common', text: common });
				common = '';
				mine = '';
				base = null;
				theirs = '';
				state = 'mine';
			} else if (current.startsWith(BASE) || current.startsWith(THEIRS)) {
				return null;
			} else {
				common += line;
			}
		} else if (state === 'mine') {
			if (current.startsWith(BASE)) {
				base = '';
				state = 'base';
			} else if (current === SEPARATOR) {
				state = 'theirs';
			} else if (current.startsWith(OURS) || current.startsWith(THEIRS)) {
				return null;
			} else {
				mine += line;
			}
		} else if (state === 'base') {
			if (current === SEPARATOR) {
				state = 'theirs';
			} else if (
				current.startsWith(OURS) ||
				current.startsWith(BASE) ||
				current.startsWith(THEIRS)
			) {
				return null;
			} else {
				base = (base ?? '') + line;
			}
		} else {
			if (current.startsWith(THEIRS)) {
				segments.push({ kind: 'hunk', mine, base, theirs });
				state = 'common';
			} else if (current.startsWith(OURS) || current.startsWith(BASE) || current === SEPARATOR) {
				return null;
			} else {
				theirs += line;
			}
		}
	}

	// 閉じ忘れ（ハンクの途中で終わった）は解析できない
	if (state !== 'common') return null;
	if (common !== '') segments.push({ kind: 'common', text: common });
	if (!segments.some((segment) => segment.kind === 'hunk')) return null;
	return { segments, eol: detectEol(lines) };
}

/** ハンクの数 */
export function hunkCount(doc: ConflictDocument): number {
	return doc.segments.filter((segment) => segment.kind === 'hunk').length;
}

/** 選択の既定値（ハンクの数だけ未選択）。すべて null のとき、解消後の全文は組み立てられない */
export function initialPicks(doc: ConflictDocument): (HunkPick | null)[] {
	return Array.from({ length: hunkCount(doc) }, () => null);
}

/** ハンクの選択 1 件を置き換えた新しい配列を返す */
export function setPick(
	picks: ReadonlyArray<HunkPick | null>,
	index: number,
	pick: HunkPick | null
): (HunkPick | null)[] {
	return picks.map((current, i) => (i === index ? pick : current));
}

/** 選択によるハンクのテキスト（両方残すは「この PC の版」の後に「クラウドの版」） */
export function pickText(hunk: HunkSegment, pick: HunkPick): string {
	switch (pick.kind) {
		case 'mine':
			return hunk.mine;
		case 'theirs':
			return hunk.theirs;
		case 'both':
			return hunk.mine + hunk.theirs;
		case 'edited':
			return pick.text;
	}
}

/**
 * 全ハンクの選択から、解消後の全文を組み立てる。
 * 選択が足りない（null がある・ハンク数と違う）ときは null。
 * 編集したテキストが改行で終わらず、後ろに続きがあるときは eol を補う（行が連結されないため）。
 * 共通部分と「どの版も改行で終わる」ハンクは、そのまま連結する。
 */
export function composeResolved(
	doc: ConflictDocument,
	picks: ReadonlyArray<HunkPick | null>
): string | null {
	if (picks.length !== hunkCount(doc) || picks.some((pick) => pick === null)) return null;

	const parts: string[] = [];
	let index = 0;
	for (const segment of doc.segments) {
		if (segment.kind === 'common') {
			parts.push(segment.text);
			continue;
		}
		const pick = picks[index];
		index += 1;
		if (pick === null || pick === undefined) return null;
		parts.push(pickText(segment, pick));
	}

	return parts
		.map((part, i) =>
			i < parts.length - 1 && part !== '' && !part.endsWith('\n') ? part + doc.eol : part
		)
		.join('');
}
