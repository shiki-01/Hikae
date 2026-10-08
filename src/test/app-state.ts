// テスト用の `$app/state`。画面が参照する現在の URL を、テストから差し替えられるようにする
export const page = { url: new URL('http://localhost/') };

export function setPageUrl(url: string): void {
	page.url = new URL(url, 'http://localhost');
}
