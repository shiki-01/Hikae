import { goto } from '$app/navigation';

// 再ログイン（ログイン手順だけを開き、終わったら元の画面へ戻る）の行き先の組み立て

/**
 * ログインの画面（/welcome）に付ける、戻り先のクエリ（`?return=...`）。
 * 終わったら `current`（いま開いている画面）へ戻る。ログインの画面そのものにいるときは空
 */
export function reloginSearch(current: Pick<URL, 'pathname' | 'search'>): string {
	if (current.pathname === '/welcome') return '';
	return `?return=${encodeURIComponent(`${current.pathname}${current.search}`)}`;
}

/**
 * `?return=` の値のうち、アプリ内の画面を指すものだけを返す。
 * 外部の URL や、`//` とバックスラッシュ、制御文字を含む値への誘導は受け付けない。
 */
export function safeReturnPath(value: string | null): string | null {
	if (value === null || !value.startsWith('/')) return null;
	if (value.startsWith('//') || value.includes('\\')) return null;
	for (const char of value) {
		if (char.charCodeAt(0) < 0x20) return null;
	}
	return value;
}

/** ログインし直したあと、元の画面へ戻る。`path` は `safeReturnPath` を通した値だけを渡す */
export async function goBackTo(path: string): Promise<void> {
	await goto(path);
}
