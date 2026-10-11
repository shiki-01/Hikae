import type { SkipStashReason } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

/** 退避が残っている理由の文言のキー（設計書 4.8。推定の理由を、利用者の言葉にする） */
const REASON_KEYS: Record<SkipStashReason, MessageKey> = {
	pending: 'skip_stash.reason_pending',
	'deleted-upstream': 'skip_stash.reason_deleted',
	unknown: 'skip_stash.reason_unknown'
};

/** 残っている理由の文言のキー */
export function stashReasonKey(reason: SkipStashReason): MessageKey {
	return REASON_KEYS[reason];
}

/** パスの最後の名前（`/` と `\` のどちらの区切りでも） */
export function baseName(path: string): string {
	const normalized = path.replaceAll('\\', '/');
	return normalized.slice(normalized.lastIndexOf('/') + 1);
}

/** 退避した時刻（Unix 秒）を日時にする。取れなければ null */
export function stashSavedDate(savedAt: number | null): Date | null {
	return savedAt === null ? null : new Date(savedAt * 1000);
}
