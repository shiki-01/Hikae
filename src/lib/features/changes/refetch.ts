/** ファイル監視が使えないときのフォールバックとして、変更一覧を取り直す間隔（ms） */
export const CHANGES_FALLBACK_INTERVAL_MS = 30_000;

/**
 * 変更一覧を定期的に取り直す間隔。取り直さないときは false。
 *
 * - ブラウザ単体（モック）では取り直さない
 * - Tauri 実行時でファイル監視が動いている間は取り直さない（変更はバックエンドの通知で届く）
 * - 監視が動いていない（失敗・未開始・非対応）ときだけ、フォールバックとして一定間隔で取り直す
 */
export function changesRefetchInterval(isTauri: boolean, watching: boolean): number | false {
	if (!isTauri) return false;
	return watching ? false : CHANGES_FALLBACK_INTERVAL_MS;
}
