import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import type { AppSettings } from '#lib/api/types.js';
import {
	SETTINGS_TABS,
	SETTING_ROWS,
	applyPatch,
	confirmationFor,
	optionValue
} from './settings-model';
import type { SelectRow } from './settings-model';

const current: AppSettings = {
	pullOnStartup: true,
	pullIntervalMinutes: 15,
	saveBeforePull: true,
	conflictMode: 'notify-only',
	autoPushAfterSave: true,
	pushReminderHours: 24,
	autoSnapshotEnabled: true,
	autoSnapshotDelaySecs: 120,
	snapshotRetentionDays: 90,
	autoSaveAfterRestore: true,
	largeFileWarnMb: 50,
	showSnapshotsInTimeline: 'collapsed'
};

describe('設定項目の定義', () => {
	it('一般・取り込み・アップロード・自動保存の 4 つのタブがある', () => {
		expect(SETTINGS_TABS).toEqual(['general', 'fetch', 'push', 'auto_save']);
	});

	it('同じ項目を 2 か所に出さない', () => {
		const keys = SETTINGS_TABS.flatMap((tab) => SETTING_ROWS[tab].map((row) => row.key));
		expect(new Set(keys).size).toBe(keys.length);
	});

	it('バックエンドが参照していない設定は画面に出さない', () => {
		const keys = SETTINGS_TABS.flatMap((tab) => SETTING_ROWS[tab].map((row) => row.key)).sort();
		expect(keys).toEqual(
			[
				'autoPushAfterSave',
				'largeFileWarnMb',
				'pullIntervalMinutes',
				'pullOnStartup',
				'saveBeforePull',
				'showSnapshotsInTimeline',
				'snapshotRetentionDays'
			].sort()
		);
	});

	it('ラベルと説明の文言が定義されている', () => {
		for (const tab of SETTINGS_TABS) {
			for (const row of SETTING_ROWS[tab]) {
				expect(t(row.label)).not.toBe('');
				expect(t(row.text)).not.toBe('');
			}
		}
	});

	it('選択肢に現在の既定値を含む', () => {
		for (const tab of SETTINGS_TABS) {
			for (const row of SETTING_ROWS[tab]) {
				if (row.type !== 'select') continue;
				expect(row.options.map((o) => o.value)).toContain(current[row.key]);
			}
		}
	});
});

describe('選択肢の値の復元', () => {
	const row = SETTING_ROWS.fetch.find((r) => r.key === 'pullIntervalMinutes') as SelectRow;

	it('文字列から数値の選択肢を引く', () => {
		expect(optionValue(row, '5')).toBe(5);
		expect(optionValue(row, '0')).toBe(0);
	});

	it('選択肢に無い値は undefined', () => {
		expect(optionValue(row, '7')).toBeUndefined();
	});
});

describe('確認ダイアログが必要な変更', () => {
	it('自動アップロードをオフにするときだけ確認する', () => {
		expect(confirmationFor('autoPushAfterSave', current, false)).not.toBeNull();
		expect(confirmationFor('autoPushAfterSave', current, true)).toBeNull();
	});

	it('自動保存を残す期間を短くするときだけ確認する', () => {
		expect(confirmationFor('snapshotRetentionDays', current, 30)).not.toBeNull();
		expect(confirmationFor('snapshotRetentionDays', current, 365)).toBeNull();
		expect(confirmationFor('snapshotRetentionDays', current, 90)).toBeNull();
	});

	it('そのほかの変更は確認しない', () => {
		expect(confirmationFor('pullOnStartup', current, false)).toBeNull();
		expect(confirmationFor('largeFileWarnMb', current, 25)).toBeNull();
	});
});

describe('更新後の設定の先行反映', () => {
	it('全体設定では上書き項目を増やさない', () => {
		const next = applyPatch({ settings: current, overridden: [] }, { pullOnStartup: false }, false);
		expect(next.settings.pullOnStartup).toBe(false);
		expect(next.overridden).toEqual([]);
	});

	it('プロジェクト別では変更した項目を上書き済みにし、重複させない', () => {
		const first = applyPatch({ settings: current, overridden: [] }, { largeFileWarnMb: 25 }, true);
		const second = applyPatch(first, { largeFileWarnMb: 100, pullIntervalMinutes: 0 }, true);
		expect(second.overridden).toEqual(['largeFileWarnMb', 'pullIntervalMinutes']);
		expect(second.settings.largeFileWarnMb).toBe(100);
	});
});
