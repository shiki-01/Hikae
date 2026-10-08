/** 親フォルダに名前をつないだパスを返す。区切り文字は親フォルダの書き方に合わせる */
export function joinPath(parent: string, name: string): string {
	const trimmed = parent.replace(/[\\/]+$/, '');
	const separator = trimmed.includes('\\') && !trimmed.includes('/') ? '\\' : '/';
	return `${trimmed}${separator}${name}`;
}
