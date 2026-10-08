import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { NUMBER_RANGES } from './number-input';
import type { AppSettings } from '#lib/api/types.js';
import {
	SETTINGS_TABS,
	SETTING_ROWS,
	applyPatch,
	CUSTOM_OPTION,
	confirmationFor,
	matchesPreset,
	numberSelectValue,
	optionValue
} from './settings-model';
import type { NumberRow, SelectRow } from './settings-model';

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
				'autoSnapshotDelaySecs',
				'autoSnapshotEnabled',
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

	it('選択肢（数値の項目は選択肢とカスタム）に現在の既定値を含む', () => {
		for (const tab of SETTINGS_TABS) {
			for (const row of SETTING_ROWS[tab]) {
				if (row.type === 'toggle') continue;
				if (row.type === 'number') {
					expect(row.presets.map((p) => p.value)).toContain(current[row.key]);
					continue;
				}
				expect(row.options.map((o) => o.value)).toContain(current[row.key]);
			}
		}
	});
});

describe('選択肢の値の復元', () => {
	const row = SETTING_ROWS.general.find((r) => r.key === 'showSnapshotsInTimeline') as SelectRow;

	it('文字列から選択肢の値を引く', () => {
		expect(optionValue(row, 'shown')).toBe('shown');
		expect(optionValue(row, 'hidden')).toBe('hidden');
	});

	it('選択肢に無い値は undefined', () => {
		expect(optionValue(row, 'unknown')).toBeUndefined();
	});
});

describe('数値の項目（選択肢 + カスタム）', () => {
	const rows = SETTINGS_TABS.flatMap((tab) => SETTING_ROWS[tab]).filter(
		(row): row is NumberRow => row.type === 'number'
	);
	const byKey = (key: string) => rows.find((r) => r.key === key) as NumberRow;

	it('取り込み間隔・自動保存の待ち時間・保持期間・警告サイズがカスタムできる', () => {
		expect(rows.map((r) => r.key).sort()).toEqual([
			'autoSnapshotDelaySecs',
			'largeFileWarnMb',
			'pullIntervalMinutes',
			'snapshotRetentionDays'
		]);
	});

	it('選択肢の値はすべてカスタムの範囲に収まる（0 の「オフ」だけは範囲外）', () => {
		for (const row of rows) {
			const range = NUMBER_RANGES[row.key];
			for (const preset of row.presets) {
				const inRange = preset.value >= range.min && preset.value <= range.max;
				expect(inRange || preset.value === 0, `${row.key}: ${preset.value}`).toBe(true);
			}
			expect(t(row.unit)).not.toBe('');
		}
	});

	it('現在の値が選択肢にあれば選択肢を、なければ「カスタム」を選んだ状態にする', () => {
		const row = byKey('autoSnapshotDelaySecs');
		expect(matchesPreset(row, 120)).toBe(true);
		expect(numberSelectValue(row, 120, false)).toBe('120');
		expect(numberSelectValue(row, 45, false)).toBe(CUSTOM_OPTION);
		// 「カスタム」を選んだ直後は、値が選択肢と同じでも入力欄を出す
		expect(numberSelectValue(row, 120, true)).toBe(CUSTOM_OPTION);
	});

	it('取り込み間隔の 0 は「オフ」の選択肢として選べる', () => {
		const row = byKey('pullIntervalMinutes');
		expect(matchesPreset(row, 0)).toBe(true);
		expect(numberSelectValue(row, 0, false)).toBe('0');
	});
});

describe('確認ダイアログが必要な変更', () => {
	it('自動アップロードをオフにするときだけ確認する', () => {
		expect(confirmationFor('autoPushAfterSave', current, false)).not.toBeNull();
		expect(confirmationFor('autoPushAfterSave', current, true)).toBeNull();
	});

	it('自動保存をオフにするときだけ確認する', () => {
		expect(confirmationFor('autoSnapshotEnabled', current, false)).not.toBeNull();
		expect(confirmationFor('autoSnapshotEnabled', current, true)).toBeNull();
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
