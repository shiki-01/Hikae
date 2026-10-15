// モバイル版の操作（ダウンロード）。失敗は既存のエラー表示（reportError）で 3 要素にして出す。

import { mobileApi } from '#lib/api/mobile-index.js';
import { t } from '#lib/i18n/index.js';
import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
import { saveFile } from './download';

/** ファイルを端末へ保存する（共有、または <a download>）。キャンセルは何もしない */
export async function downloadEntry(
	owner: string,
	name: string,
	path: string,
	sha: string
): Promise<void> {
	try {
		const file = await mobileApi.download(owner, name, path, sha);
		const result = await saveFile(file);
		if (result !== 'cancelled') {
			pushToast({ type: 'success', message: t('mobile.download.done', { name: file.fileName }) });
		}
	} catch (error) {
		reportError(error, { retry: () => void downloadEntry(owner, name, path, sha) });
	}
}
