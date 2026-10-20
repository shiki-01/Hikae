// 作業コピー（設計書 4.9）の画面側の判定。バックエンドの検査（`normalize_display_name`）と同じ規則を持つ。
// 送る前に弾くだけで、正しさはバックエンドが保証する。

import type { WorkCopy } from '#lib/api/types.js';

/** 表示名の最大の文字数（設計書 4.9。バックエンドと同じ） */
export const DISPLAY_NAME_MAX_CHARS = 40;

/**
 * 表示名を検査し、前後の空白を除いた値を返す。1〜40 文字で、制御文字を含まないこと。
 * 外れたときは null。
 */
export function normalizeDisplayName(raw: string): string | null {
	const trimmed = raw.trim();
	const chars = [...trimmed];
	if (chars.length === 0 || chars.length > DISPLAY_NAME_MAX_CHARS) return null;
	if (chars.some((c) => /\p{Cc}/u.test(c))) return null;
	return trimmed;
}

/** 「別の案を試す」の入力が、送れる形か（入力中は空でなければ誤りの注記を出す） */
export function isInvalidDisplayName(raw: string): boolean {
	return raw.trim() !== '' && normalizeDisplayName(raw) === null;
}

/** 「この案を元のまま側に取り込む」を出せるか（いまの作業コピーが元でないとき） */
export function canAdopt(items: WorkCopy[]): boolean {
	return items.some((w) => w.isCurrent && !w.isOriginal);
}

/** 「しまう」を出せる作業コピー（いまの作業コピーと元のものは対象外） */
export function archivable(items: WorkCopy[]): WorkCopy[] {
	return items.filter((w) => !w.isCurrent && !w.isOriginal);
}
