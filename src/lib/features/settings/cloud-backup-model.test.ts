import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { cloudStatusMessage } from './cloud-backup-model';

describe('自動保存のクラウド控えの表示文', () => {
	it('控えをオフのときは、この PC にだけあると出す', () => {
		const msg = cloudStatusMessage({ enabled: false, localCount: 3, cloudCount: null });
		expect(msg.key).toBe('settings.cloud_status_local_only');
		expect(t(msg.key, msg.params)).toContain('3');
	});

	it('オンで件数が未確認（再起動後など）のときは、次のアップロードを待つ文言にする', () => {
		const msg = cloudStatusMessage({ enabled: true, localCount: 5, cloudCount: null });
		expect(msg.key).toBe('settings.cloud_status_unknown');
		expect(msg.params).toEqual({ local: 5 });
	});

	it('オンで件数が分かるときは、この PC とクラウドの件数を出す', () => {
		const msg = cloudStatusMessage({ enabled: true, localCount: 5, cloudCount: 7 });
		expect(msg.key).toBe('settings.cloud_status');
		expect(msg.params).toEqual({ local: 5, cloud: 7 });
		const text = t(msg.key, msg.params);
		expect(text).toContain('5');
		expect(text).toContain('7');
	});
});
