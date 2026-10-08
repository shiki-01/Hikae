import type { AddFilesOutcome, AddedFile, RejectedFile } from '#lib/api/types.js';
import { formatBytes } from '#lib/i18n/format.js';
import { t } from '#lib/i18n/index.js';

export interface AddFilesSummary {
	/** 追加できたファイルの数 */
	addedCount: number;
	/** 同名があったため別の名前で追加したファイル */
	renamed: AddedFile[];
	/** 大きめで、アップロードに時間がかかるファイル */
	large: AddedFile[];
	/** 追加しなかったファイル */
	rejected: RejectedFile[];
	/** 上限を超えて追加できなかったファイルがあるか（ドロップ欄を警告表示にする） */
	hasTooLarge: boolean;
	/** トーストだけでは伝えきれず、ダイアログで知らせる必要があるか */
	needsDialog: boolean;
}

export function summarizeAddFiles(outcome: AddFilesOutcome): AddFilesSummary {
	const renamed = outcome.added.filter((file) => file.renamed);
	const large = outcome.added.filter((file) => file.large);
	const hasTooLarge = outcome.rejected.some((file) => file.reason === 'too_large');
	return {
		addedCount: outcome.added.length,
		renamed,
		large,
		rejected: outcome.rejected,
		hasTooLarge,
		needsDialog: renamed.length > 0 || large.length > 0 || outcome.rejected.length > 0
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
	}
}
