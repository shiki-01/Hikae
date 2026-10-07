/**
 * プロジェクト直下の相対パスを絶対パスに解決する。
 * プロジェクト外を指す入力（絶対パス、ドライブ指定、`..` を含むもの）は null を返す。
 * シンボリックリンクの実体までは検査しない（文字列としての検査のみ）。
 */
export function resolveInProject(root: string, relative: string): string | null {
	if (root === '' || relative === '' || relative.includes('\0')) return null;
	// UNC・ドライブ指定・絶対パスは拒否する
	if (/^([\\/]|[A-Za-z]:)/.test(relative)) return null;

	const segments = relative.split(/[\\/]+/).filter((segment) => segment !== '' && segment !== '.');
	if (segments.length === 0) return null;
	if (segments.some((segment) => segment === '..')) return null;

	const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/';
	const base = root.replace(/[\\/]+$/, '');
	return [base, ...segments].join(separator);
}
