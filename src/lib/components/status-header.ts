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

/**
 * 状態表示とは別に、控えめな「取り込む」ボタンを右側に出すか。
 *
 * 状態表示は優先順位の 1 つだけ（上の `resolveStatus`）のため、別の PC の変更がある（取り込み待ち）のに
 * 未保存の変更が優先されると、取り込む操作が見えなくなる。そこで、取り込み待ちがあり、かつ
 * 未保存の変更が優先されているとき（未保存の変更が理由で自動の取り込みを見送ったときを含む）は、
 * 状態表示を変えずにボタンだけを足す。取り込み待ちが優先されている状態（`fetch_pending`）は
 * 主ボタンが「取り込む」なので足さない。ぶつかり・途中で止まった操作・処理中・オフライン・
 * 再ログインが必要なときは、取り込みを進められない（または先に対応すべきことがある）ので出さない。
 */
export function showsSecondaryFetch(
	status: StatusState,
	fetchPendingCount: number,
	attention: 'auth' | 'unsaved-changes' | null = null
): boolean {
	if (fetchPendingCount <= 0) return false;
	if (status === 'unsaved') return true;
	return status === 'attention' && attention === 'unsaved-changes';
}
