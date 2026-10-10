// pdf.js のワーカーの処理モジュール（pdf-document.ts が同じスレッドで使う）の型。
// pdfjs-dist はこのモジュールの型を含まないため、使う名前だけをここで宣言する。
declare module 'pdfjs-dist/build/pdf.worker.mjs' {
	export const WorkerMessageHandler: unknown;
}
