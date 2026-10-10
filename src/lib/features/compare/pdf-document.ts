// PDF の読み込み（pdf.js）。表示拡張（iframe）の中で使う。
// ワーカーは作らず、pdf.js のワーカーの処理（pdf.worker.mjs の WorkerMessageHandler）を同じスレッドで動かす。
// 理由: 表示拡張は sandbox="allow-scripts"（opaque origin）で、Chromium は opaque origin の文書から作る
// blob URL のワーカーを拒否する（実機相当の headless Chrome で確かめた）。pdf.js は `globalThis.pdfjsWorker` が
// あれば、ワーカーを作らずにその処理を使う。データはホストから postMessage で受け取るため、ネットワークは使わない。
import { getDocument, type PDFDocumentProxy } from 'pdfjs-dist';
import * as pdfjsWorker from 'pdfjs-dist/build/pdf.worker.mjs';

// pdf.js の既定の経路（同じスレッド）を使うための登録。読み込みより前に 1 回だけ行う
Object.assign(globalThis, { pdfjsWorker });

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
 * pdf のバイト列から文書を開く。パスワードを求められたら読まずに `encrypted` にする
 * （パスワードの入力は行わない）
 */
export async function loadPdfDocument(data: ArrayBuffer): Promise<PDFDocumentProxy> {
	let needsPassword = false;
	const task = getDocument({ data: new Uint8Array(data) });
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

/** 文書を破棄する（読み込みを解放する）。pdf.js では破棄は読み込みのタスク側から行う */
export function destroyPdfDocument(doc: PDFDocumentProxy): Promise<void> {
	return doc.loadingTask.destroy();
}
