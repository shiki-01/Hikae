// リポジトリ内のパスの扱い（パンくず・親フォルダ・結合）。パスの区切りは `/`、ルートは空文字。

export interface Crumb {
	/** 表示する名前 */
	label: string;
	/** その階層のパス（ルートからの絶対的な位置） */
	path: string;
}

/** パスを階層ごとに分ける（ルートは含まない）。`a/b` は `a` と `a/b` */
export function breadcrumbsOf(path: string): Crumb[] {
	const segments = path.split('/').filter((segment) => segment !== '');
	return segments.map((label, index) => ({
		label,
		path: segments.slice(0, index + 1).join('/')
	}));
}

/** 親のフォルダのパス。ルートなら空文字 */
export function parentPathOf(path: string): string {
	const index = path.lastIndexOf('/');
	return index < 0 ? '' : path.slice(0, index);
}

/** フォルダのパスに名前をつなぐ（フォルダが空ならルート直下） */
export function joinRepoPath(folder: string, name: string): string {
	return folder === '' ? name : `${folder}/${name}`;
}
