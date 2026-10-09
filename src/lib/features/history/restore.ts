import type { RestoreScope } from '#lib/api/types.js';
import { fileNameOf } from '#lib/features/changes/discard.js';
import { formatDateTime } from '#lib/i18n/format.js';
import { t } from '#lib/i18n/index.js';

export function scopeKey(scope: RestoreScope): string {
	return scope.kind === 'all' ? 'all' : `file:${scope.path}`;
}

/**
 * 「元に戻した後に自動で保存」で残る保存のメモ（例:「10/4 18:02 の状態に戻しました」）。
 * 文言は画面側で決め、バックエンドは設定がオンのときだけ、そのまま使う。
 */
export function restoreSaveMemo(scope: RestoreScope, when: Date): string {
	const time = formatDateTime(when);
	return scope.kind === 'file'
		? t('restore.save_memo_file', { name: fileNameOf(scope.path), time })
		: t('restore.save_memo', { time });
}
