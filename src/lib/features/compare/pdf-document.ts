// PDF の読み込み（pdf.js）。ワーカーは Vite の `?url` で同梱し、外部の CDN は使わない。
import { getDocument, GlobalWorkerOptions, type PDFDocumentProxy } from 'pdfjs-dist';
import pdfWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

/** 読み込みの失敗の理由。暗号化（パスワード）と、それ以外（壊れている・読めない）を分ける */
export type PdfFailure = 'encrypted' | 'failed';

export class PdfLoadError extends Error {
	readonly reason: PdfFailure;

	constructor(reason: PdfFailure) {
		super(reason);
		this.name = 'PdfLoadError';
		this.reason = reason;
	}
}

/**
 * app-blob の URL から PDF を読む。パスワードを求められたら読まずに `encrypted` にする
 * （パスワードの入力は行わない）。
 */
export async function loadPdfDocument(url: string): Promise<PDFDocumentProxy> {
	const response = await fetch(url);
	if (!response.ok) throw new PdfLoadError('failed');
	const data = await response.arrayBuffer();
	let needsPassword = false;
	const task = getDocument({ data });
	task.onPassword = () => {
		needsPassword = true;
		void task.destroy();
	};
	try {
		return await task.promise;
	} catch (error) {
		const passwordError = error instanceof Error && error.name === 'PasswordException';
		throw new PdfLoadError(needsPassword || passwordError ? 'encrypted' : 'failed');
	}
}

/** 文書を破棄する（ワーカーと読み込みを解放する）。pdf.js では破棄は読み込みのタスク側から行う */
export function destroyPdfDocument(doc: PDFDocumentProxy): Promise<void> {
	return doc.loadingTask.destroy();
}
