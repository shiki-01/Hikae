// 拡張子から MIME の型と、クリップボードの画像の拡張子を決める純関数。

const MIME_BY_EXTENSION: Record<string, string> = {
	txt: 'text/plain',
	md: 'text/markdown',
	csv: 'text/csv',
	json: 'application/json',
	png: 'image/png',
	jpg: 'image/jpeg',
	jpeg: 'image/jpeg',
	gif: 'image/gif',
	webp: 'image/webp',
	svg: 'image/svg+xml',
	pdf: 'application/pdf',
	docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
	xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
	pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
	zip: 'application/zip'
};

const EXTENSION_BY_MIME: Record<string, string> = {
	'image/png': 'png',
	'image/jpeg': 'jpg',
	'image/gif': 'gif',
	'image/webp': 'webp'
};

/** 名前の拡張子（小文字。ドットを除く）。無ければ空文字 */
export function extensionOf(name: string): string {
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return '';
	return name.slice(dot + 1).toLowerCase();
}

/** 名前から MIME の型を決める。分からなければ application/octet-stream */
export function mimeOf(name: string): string {
	return MIME_BY_EXTENSION[extensionOf(name)] ?? 'application/octet-stream';
}

/** 画像の MIME の型から拡張子を決める。対応しない型は null */
export function extensionOfImageMime(mime: string): string | null {
	return EXTENSION_BY_MIME[mime] ?? null;
}
