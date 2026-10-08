import type { FileKind } from '#lib/api/types.js';

const KINDS: Record<Exclude<FileKind, 'unknown'>, readonly string[]> = {
	document: ['doc', 'docx', 'odt', 'rtf', 'pages', 'ppt', 'pptx', 'odp', 'key'],
	table: ['xls', 'xlsx', 'xlsm', 'csv', 'tsv', 'ods', 'numbers'],
	image: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'heic', 'tif', 'tiff', 'ico', 'psd'],
	text: [
		'txt',
		'md',
		'markdown',
		'json',
		'xml',
		'yml',
		'yaml',
		'log',
		'ini',
		'toml',
		'html',
		'css'
	],
	pdf: ['pdf']
};

/** 拡張子から、ファイルの種類（画面に出す分類）を決める。分からなければ `unknown` */
export function fileKindOf(path: string): FileKind {
	const name = path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return 'unknown';
	const extension = name.slice(dot + 1).toLowerCase();
	for (const [kind, extensions] of Object.entries(KINDS)) {
		if (extensions.includes(extension)) return kind as FileKind;
	}
	return 'unknown';
}
