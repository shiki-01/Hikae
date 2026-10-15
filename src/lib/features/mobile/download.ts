// 端末への保存（ダウンロード）。共有の機能を優先し、使えなければ <a download> で保存する。
// 端末ごとの動作は未確認（docs/design.md 14.7）。

import { base64ToBytes } from './base64';

/** 保存の結果。キャンセルは失敗ではない */
export type SaveResult = 'shared' | 'downloaded' | 'cancelled';

export async function saveFile(input: {
	fileName: string;
	mime: string;
	contentBase64: string;
}): Promise<SaveResult> {
	const blob = new Blob([base64ToBytes(input.contentBase64).buffer as ArrayBuffer], {
		type: input.mime
	});
	const file = new File([blob], input.fileName, { type: input.mime });
	const nav = globalThis.navigator;
	if (nav && typeof nav.share === 'function' && nav.canShare?.({ files: [file] })) {
		try {
			await nav.share({ files: [file], title: input.fileName });
			return 'shared';
		} catch (error) {
			if (error instanceof Error && error.name === 'AbortError') return 'cancelled';
			// 共有が使えなかったときは、下の保存に切り替える
		}
	}
	const url = URL.createObjectURL(blob);
	try {
		const link = globalThis.document.createElement('a');
		link.href = url;
		link.download = input.fileName;
		globalThis.document.body.appendChild(link);
		link.click();
		link.remove();
	} finally {
		// 保存の開始を待つため、少し後に解放する
		setTimeout(() => URL.revokeObjectURL(url), 1000);
	}
	return 'downloaded';
}
