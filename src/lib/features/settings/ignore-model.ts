import type { IgnoreKind, IgnoreRule } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

/** 追加ダイアログで選べる種類（表示文言のキーと、入力欄の案内） */
export const IGNORE_KIND_OPTIONS: {
	value: IgnoreKind;
	label: MessageKey;
	inputLabel: MessageKey;
	placeholder: MessageKey;
	hint: MessageKey;
}[] = [
	{
		value: 'extension',
		label: 'ignore.kind_extension',
		inputLabel: 'ignore.input_label_extension',
		placeholder: 'ignore.placeholder_extension',
		hint: 'ignore.hint_extension'
	},
	{
		value: 'folder',
		label: 'ignore.kind_folder',
		inputLabel: 'ignore.input_label_folder',
		placeholder: 'ignore.placeholder_folder',
		hint: 'ignore.hint_folder'
	},
	{
		value: 'file',
		label: 'ignore.kind_file',
		inputLabel: 'ignore.input_label_file',
		placeholder: 'ignore.placeholder_file',
		hint: 'ignore.hint_file'
	}
];

/**
 * 入力欄の値を、バックエンドへ渡す形にそろえる。
 * 書き方（`*.ext` など）の組み立てと、使えない文字の検査はバックエンドが行う。
 * - 拡張子: 前後の空白と先頭の `.` を外す（`.psd` も `psd` と同じに扱う）
 * - フォルダ・ファイル: 前後の空白を外し、`\` を `/` にして、前後の `/` を外す
 */
export function normalizeIgnoreInput(kind: IgnoreKind, raw: string): string {
	const trimmed = raw.trim();
	if (kind === 'extension') return trimmed.replace(/^\.+/, '');
	return trimmed.replaceAll('\\', '/').replace(/^\/+|\/+$/g, '');
}

/** 追加できる入力か（空でなければ送る） */
export function canSubmitIgnoreInput(kind: IgnoreKind, raw: string): boolean {
	return normalizeIgnoreInput(kind, raw).length > 0;
}

/** 一覧の 1 行の状態。件数を調べられない書き方の行は `unknown` */
export type IgnoreRuleStatus = 'tracked' | 'none' | 'unknown';

export function ignoreRuleStatus(rule: IgnoreRule): IgnoreRuleStatus {
	if (rule.trackedCount === null) return 'unknown';
	return rule.trackedCount > 0 ? 'tracked' : 'none';
}

/** 保存されているファイルの一覧の表示用の要約。長い一覧は先頭だけ出し、残りの件数を返す */
export interface TrackedSummary {
	/** 画面に出すパス（区切りを `/` にそろえる） */
	shown: string[];
	/** 出し切れなかった件数 */
	rest: number;
}

export function summarizeTracked(paths: string[], limit = 20): TrackedSummary {
	const normalized = paths.map((path) => path.replaceAll('\\', '/'));
	return {
		shown: normalized.slice(0, limit),
		rest: Math.max(0, normalized.length - limit)
	};
}
