export type StatusState =
	| 'conflict'
	| 'interrupted'
	| 'large_files'
	| 'syncing'
	| 'offline'
	| 'attention'
	| 'unsaved'
	| 'fetch_pending'
	| 'push_pending'
	| 'not_connected'
	| 'saved';

export function resolveStatus(
	hasConflict: boolean,
	isSyncing: boolean,
	isOnline: boolean,
	unsavedCount: number,
	fetchPendingCount: number,
	uploadPendingCount: number,
	hasAttention = false,
	hasInterrupted = false,
	hasLargeFiles = false,
	notConnected = false
): StatusState {
	if (hasConflict) return 'conflict';
	if (hasInterrupted) return 'interrupted';
	if (hasLargeFiles) return 'large_files';
	if (isSyncing) return 'syncing';
	if (!isOnline) return 'offline';
	if (hasAttention) return 'attention';
	if (unsavedCount > 0) return 'unsaved';
	if (fetchPendingCount > 0) return 'fetch_pending';
	if (uploadPendingCount > 0) return 'push_pending';
	// 保存先（GitHub）に接続していない。差分も未保存も無いときに、「すべて保存済み」の代わりに知らせる
	if (notConnected) return 'not_connected';
	return 'saved';
}
