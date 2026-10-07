export type StatusState =
	| 'conflict'
	| 'syncing'
	| 'offline'
	| 'attention'
	| 'unsaved'
	| 'fetch_pending'
	| 'push_pending'
	| 'saved';

export function resolveStatus(
	hasConflict: boolean,
	isSyncing: boolean,
	isOnline: boolean,
	unsavedCount: number,
	fetchPendingCount: number,
	uploadPendingCount: number,
	hasAttention = false
): StatusState {
	if (hasConflict) return 'conflict';
	if (isSyncing) return 'syncing';
	if (!isOnline) return 'offline';
	if (hasAttention) return 'attention';
	if (unsavedCount > 0) return 'unsaved';
	if (fetchPendingCount > 0) return 'fetch_pending';
	if (uploadPendingCount > 0) return 'push_pending';
	return 'saved';
}
