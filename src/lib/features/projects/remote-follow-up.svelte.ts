import type { RemoteFollowUp } from './remote-follow-up';

/**
 * 保存先の作成・接続の続き（確認ダイアログ）の状態。
 * 追加ダイアログは閉じて画面が移ることがあるため、画面の部品ではなく共有の状態に持つ。
 * `version` は続きが差し替わるたびに増える（処理の完了時に、別の続きへ置き換わったかを見分ける）。
 */
export const remoteFollowUp: { current: RemoteFollowUp | null; version: number } = $state({
	current: null,
	version: 0
});

export function startRemoteFollowUp(next: RemoteFollowUp): void {
	remoteFollowUp.version += 1;
	remoteFollowUp.current = next;
}

export function clearRemoteFollowUp(): void {
	remoteFollowUp.current = null;
}
