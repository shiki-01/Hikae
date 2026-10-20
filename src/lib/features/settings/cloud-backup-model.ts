import type { CloudBackupStatus } from '#lib/api/types.js';
import type { MessageKey, MessageParams } from '#lib/i18n/index.js';

/**
 * 自動保存のクラウド控えの状態を、画面に出す文言の鍵と値にする（設計書 6.4）。
 * 控えをオフのときは「この PC にだけある」、オンで件数が未確認のときは次のアップロードを待つ文言にする。
 */
export function cloudStatusMessage(status: CloudBackupStatus): {
	key: MessageKey;
	params: MessageParams;
} {
	const local = status.localCount;
	if (!status.enabled) {
		return { key: 'settings.cloud_status_local_only', params: { local } };
	}
	if (status.cloudCount === null) {
		return { key: 'settings.cloud_status_unknown', params: { local } };
	}
	return { key: 'settings.cloud_status', params: { local, cloud: status.cloudCount } };
}
