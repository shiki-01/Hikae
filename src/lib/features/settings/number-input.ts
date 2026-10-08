/**
 * 数値の設定の「カスタム」入力の検証（設計書 7章、7.2）。
 * 範囲は、バックエンドの検証（`src-tauri/crates/core-store/src/settings.rs` の
 * `PULL_INTERVAL_MINUTES` ほか）と一致させる。範囲外の値は画面で止めて保存しない。
 */

export interface NumberRange {
	min: number;
	max: number;
}

/** カスタムで入力できる範囲。取り込み間隔の 0（オフ）は選択肢からだけ選べる */
export const NUMBER_RANGES = {
	pullIntervalMinutes: { min: 1, max: 1440 },
	autoSnapshotDelaySecs: { min: 10, max: 3600 },
	snapshotRetentionDays: { min: 7, max: 730 },
	largeFileWarnMb: { min: 1, max: 100 }
} as const satisfies Record<string, NumberRange>;

export type NumberSettingKey = keyof typeof NUMBER_RANGES;

export type ParsedNumber = { ok: true; value: number } | { ok: false };

/**
 * 入力欄の文字列を整数として読む。全角の数字（日本語入力のまま）や前後の空白は受け付ける。
 * 空、小数、符号、単位つきの文字、範囲外は `ok: false`。
 */
export function parseCustomNumber(raw: string, range: NumberRange): ParsedNumber {
	const text = raw.normalize('NFKC').trim();
	if (!/^\d{1,9}$/.test(text)) return { ok: false };
	const value = Number(text);
	if (value < range.min || value > range.max) return { ok: false };
	return { ok: true, value };
}
