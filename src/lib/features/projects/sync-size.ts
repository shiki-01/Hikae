import type { SizeCheck } from '#lib/api/types.js';

/** 大きいファイルのため、取り込み・アップロードを何も実行せずに見送ったときの内容 */
export interface SyncSizeRequest {
	kind: 'fetch' | 'push';
	check: SizeCheck;
}

/** 結果に大きいファイルの検査結果があれば、ダイアログに出す内容にする。なければ null */
export function syncSizeRequest(
	kind: SyncSizeRequest['kind'],
	result: { sizeCheck: SizeCheck | null }
): SyncSizeRequest | null {
	return result.sizeCheck ? { kind, check: result.sizeCheck } : null;
}
