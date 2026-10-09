// 画像比較で読む `app-blob` の URL（設計書 9.3）。純関数。
// 形式は `app-blob://localhost/<プロジェクト ID>/<rev>/<URL エンコードしたパス>`。
// Windows の WebView は `app-blob://` を `http://app-blob.localhost/` に変換して読むため、
// 変換の規則（Tauri の convertFileSrc と同じ）に合わせて組み立てる。

/** 作業フォルダ（「いま」）を表す画面の ID */
const CURRENT_POINT = 'current';

/** 画面の時点 ID を `app-blob` の rev にする（「いま」は作業フォルダ `work`） */
export function revForPoint(pointId: string): string {
	return pointId === CURRENT_POINT ? 'work' : pointId;
}

/** `app-blob` の URL を組み立てる。`windows` は Windows の WebView 向けの形にするか */
export function blobUrl(projectId: string, rev: string, path: string, windows: boolean): string {
	const encodedPath = encodeURIComponent(path);
	const base = windows ? 'http://app-blob.localhost' : 'app-blob://localhost';
	return `${base}/${encodeURIComponent(projectId)}/${encodeURIComponent(rev)}/${encodedPath}`;
}

/** 実行環境が Windows か（Tauri の convertFileSrc と同じ判定） */
export function isWindowsPlatform(): boolean {
	return typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');
}
