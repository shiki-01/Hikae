/** パスの最後の名前（ダイアログの見出しとトーストに出す。フォルダ名は出さない） */
export function fileNameOf(path: string): string {
	return path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
}
