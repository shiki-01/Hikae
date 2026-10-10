// ファイルの内容の表示（プレビュー）の判定と文言のキー。DOM に依存しない純関数。

import type { MessageKey } from '#lib/i18n/index.js';
import type { FilePreview, PreviewUnavailableReason } from '#lib/api/types.js';

/** PDF を縦に並べて描く上限のページ数。超えた分は描かず、注記する */
export const PDF_MAX_PAGES = 300;

/** PDF のページ数から、描くページ数と上限を超えたかを決める */
export function pdfPageWindow(total: number): { shown: number; capped: boolean } {
	return { shown: Math.min(total, PDF_MAX_PAGES), capped: total > PDF_MAX_PAGES };
}

/** 一部のみを表示しているか（抽出の上限を超えた分を省いたとき） */
export function isTruncatedPreview(preview: FilePreview): boolean {
	switch (preview.kind) {
		case 'text':
		case 'docx':
		case 'pptx':
			return preview.truncated;
		case 'xlsx':
			return preview.sheets.some((sheet) => sheet.truncated);
		case 'unavailable':
		case 'not_found':
			return false;
	}
}

/** 内容を表示できない理由ごとの文言のキー（何が起きたか） */
const UNAVAILABLE_KEYS: Record<PreviewUnavailableReason, MessageKey> = {
	too_large: 'preview.unavailable.too_large',
	unsupported_format: 'preview.unavailable.unsupported_format',
	corrupt: 'preview.unavailable.corrupt',
	binary: 'preview.unavailable.binary'
};

export function unavailableMessageKey(reason: PreviewUnavailableReason): MessageKey {
	return UNAVAILABLE_KEYS[reason];
}
