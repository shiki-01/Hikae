import { goto } from '$app/navigation';
import { resolve } from '$app/paths';

// 再ログイン（ログイン手順だけを開き、終わったら元の画面へ戻る）の行き先の組み立て

/**
 * ログインの画面（/welcome）に付ける、戻り先のクエリ（`?return=...`）。
 * 終わったら `current`（いま開いている画面）へ戻る。ログインの画面そのものにいるときは空
 */
export function reloginSearch(current: Pick<URL, 'pathname' | 'search'>): string {
	if (current.pathname === '/welcome') return '';
	return `?return=${encodeURIComponent(`${current.pathname}${current.search}`)}`;
}

function pathnameOf(path: string): string {
	return path.split(/[?#]/, 1)[0];
}

/**
 * `?return=` の値のうち、アプリ内の画面を指すものだけを返す。
 * 外部の URL や、`//` とバックスラッシュ、制御文字を含む値への誘導は受け付けない。
 * ログインの画面そのもの（往復し続ける）も受け付けない。
 */
export function safeReturnPath(value: string | null): string | null {
	if (value === null || !value.startsWith('/')) return null;
	if (value.startsWith('//') || value.includes('\\')) return null;
	for (const char of value) {
		if (char.charCodeAt(0) < 0x20) return null;
	}
	if (pathnameOf(value) === '/welcome') return null;
	return value;
}

/**
 * ログインし終えたあとの実際の行き先。設定画面（ログアウト直後に戻ると、ログインとの
 * 往復になりうる）はその設定を開いたプロジェクト、無ければ一覧（/）へ、ログインの画面は一覧へ向ける
 */
export function resolveReloginTarget(path: string): string {
	const pathname = pathnameOf(path);
	if (pathname === '/welcome') return '/';
	if (pathname === '/settings') return resolveSettingsBackTarget(path.slice(pathname.length));
	return path;
}

/**
 * ログインし直したあと、元の画面へ戻る。`path` は `safeReturnPath` を通した値だけを渡す。
 * 履歴に積まず置き換える（ログインの画面が履歴に残らない）
 */
export async function goBackTo(path: string): Promise<void> {
	await goto(resolveReloginTarget(path), { replaceState: true });
}

/** ログアウト後などに、ログインの画面へ移る。移る前の画面を履歴に残さない */
export async function gotoLogin(search: string): Promise<void> {
	await goto(`${resolve('/welcome')}${search}`, { replaceState: true });
}

/**
 * 設定画面の「戻る」の行き先。プロジェクトを開いて来たとき（`?project=`）はその画面、
 * そうでなければ一覧（/）。履歴（`history.back()`）には頼らない
 */
export function resolveSettingsBackTarget(search: string): string {
	const project = new URLSearchParams(search).get('project');
	return project ? `/project?id=${encodeURIComponent(project)}` : '/';
}

/** 設定画面を閉じて、元の画面へ戻る（履歴を置き換える） */
export async function goBackFromSettings(search: string): Promise<void> {
	await goto(resolveSettingsBackTarget(search), { replaceState: true });
}
