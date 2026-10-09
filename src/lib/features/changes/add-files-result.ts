import type { AddFilesOutcome, AddedFile, RejectedFile, SkippedFile } from '#lib/api/types.js';
import { formatBytes } from '#lib/i18n/format.js';
import { t } from '#lib/i18n/index.js';

/** 結果のダイアログに、リンクを 1 件ずつ並べる上限（残りは件数だけにする） */
export const SKIPPED_LIST_LIMIT = 20;

export interface AddFilesSummary {
	/** 追加できたファイルの数（置き換えたものを含む） */
	addedCount: number;
	/** 同名の既存ファイルを置き換えたもの */
	replaced: AddedFile[];
	/** 置き換えを選ばれたが、安全に置き換えられなかったため別名にしたもの */
	replaceRefused: AddedFile[];
	/** 同名があったため別の名前で追加したもの（置き換えられなかったものを除く） */
	renamed: AddedFile[];
	/** 大きめで、アップロードに時間がかかるファイル */
	large: AddedFile[];
	/** 追加しなかったファイル（理由つき） */
	rejected: RejectedFile[];
	/** たどらずに飛ばしたリンク（先頭から上限まで） */
	skippedLinks: SkippedFile[];
	/** 飛ばしたリンクの総数 */
	skippedLinkCount: number;
	/** 飛ばした隠しファイル・一時ファイルの数 */
	skippedHiddenCount: number;
	/** 上限を超えて追加できなかったファイルがあるか（ドロップ欄を警告表示にする） */
	hasTooLarge: boolean;
	/** トーストだけでは伝えきれず、ダイアログで知らせる必要があるか */
	needsDialog: boolean;
}

export function summarizeAddFiles(outcome: AddFilesOutcome): AddFilesSummary {
	const replaced = outcome.added.filter((file) => file.replaced);
	const replaceRefused = outcome.added.filter((file) => !file.replaced && file.replaceRefused);
	const renamed = outcome.added.filter(
		(file) => !file.replaced && file.renamed && !file.replaceRefused
	);
	const large = outcome.added.filter((file) => file.large);
	const hasTooLarge = outcome.rejected.some((file) => file.reason === 'too_large');
	const links = outcome.skipped.filter((item) => item.reason === 'link');
	const skippedHiddenCount = outcome.skipped.length - links.length;
	return {
		addedCount: outcome.added.length,
		replaced,
		replaceRefused,
		renamed,
		large,
		rejected: outcome.rejected,
		skippedLinks: links.slice(0, SKIPPED_LIST_LIMIT),
		skippedLinkCount: links.length,
		skippedHiddenCount,
		hasTooLarge,
		// 置き換え・別名・大きいファイル・断ったもの・リンクは、利用者が知っておくべき内容
		needsDialog:
			replaced.length > 0 ||
			replaceRefused.length > 0 ||
			renamed.length > 0 ||
			large.length > 0 ||
			outcome.rejected.length > 0 ||
			links.length > 0
	};
}

/** 追加できなかった理由の文言 */
export function rejectReasonText(file: RejectedFile): string {
	switch (file.reason) {
		case 'too_large':
			return file.size === null
				? t('add_files.reason_too_large_unknown')
				: t('add_files.reason_too_large', { size: formatBytes(file.size) });
		case 'not_a_file':
			return t('add_files.reason_not_a_file');
		case 'unreadable':
			return t('add_files.reason_unreadable');
		case 'in_use':
			return t('add_files.reason_in_use');
	}
}
