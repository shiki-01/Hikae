// 保存欄の下書き（要約と詳細）の純関数（設計書 4.1・10.6）。
// 要約は 1 行、詳細は任意の複数行。保存するメモは「要約」＋（詳細があれば）空行と「詳細」。
// 詳細は履歴に残り、アップロードされる。上限の値はバックエンドの検証（4,000 文字・100 行）より小さい目安。

/** 詳細の目安の文字数 */
export const DETAIL_MAX_CHARS = 1000;
/** 詳細の目安の行数 */
export const DETAIL_MAX_LINES = 20;

/** 保存欄の下書き */
export interface MemoDraft {
	summary: string;
	detail: string;
}

/** 保存するメモの文字列。要約と詳細を空行でつなぐ（詳細が空なら要約だけ） */
export function composeMemo(summary: string, detail: string): string {
	const head = summary.trim();
	const tail = detail.trim();
	if (tail === '') return head;
	return head === '' ? tail : `${head}\n\n${tail}`;
}

/** 詳細の目安（文字数と行数）と、上限を超えているか */
export function detailMeter(detail: string): { chars: number; lines: number; over: boolean } {
	const trimmed = detail.trim();
	const chars = Array.from(trimmed).length;
	const lines = trimmed === '' ? 0 : trimmed.split('\n').length;
	return {
		chars,
		lines,
		over: chars > DETAIL_MAX_CHARS || lines > DETAIL_MAX_LINES
	};
}

/**
 * AI の提案を下書きに入れる。要約は要約欄へ、本文は詳細欄へ入れる（本文が無いときは詳細を空にする）。
 * 戻り値の `backup` は、入れる前の下書き（「元に戻す」に使う）。
 */
export function applySuggestion(
	current: MemoDraft,
	suggestion: { title: string; body: string | null }
): { next: MemoDraft; backup: MemoDraft } {
	return {
		backup: { ...current },
		next: { summary: suggestion.title, detail: suggestion.body ?? '' }
	};
}

/** メモの 1 行目（要約）。前後の空白を除き、空の行は飛ばす。履歴の一覧などに使う */
export function summaryLine(message: string): string {
	return (
		message
			.split('\n')
			.map((line) => line.trim())
			.find((line) => line !== '') ?? ''
	);
}

/** メモの本文（1 行目の要約の後の行）。前後の空行を除く。要約だけのメモは空文字 */
export function memoBody(message: string): string {
	const lines = message.split('\n');
	const first = lines.findIndex((line) => line.trim() !== '');
	if (first < 0) return '';
	return lines
		.slice(first + 1)
		.join('\n')
		.replace(/^\s*\n/, '')
		.trimEnd();
}
