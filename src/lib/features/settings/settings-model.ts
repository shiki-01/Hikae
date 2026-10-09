import type { AppSettings, SettingKey, SettingsView } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';
import type { NumberSettingKey } from './number-input';

/** 設定画面で項目を並べるタブ（保存しないファイル・AI・拡張機能・詳細は別の段階で追加する） */
export type SettingsTab = 'general' | 'fetch' | 'push' | 'auto_save';

export type SettingValue = boolean | number | string;

export interface ToggleRow {
	type: 'toggle';
	key: SettingKey;
	label: MessageKey;
	text: MessageKey;
}

export interface SelectOptionRow {
	value: number | string;
	label: MessageKey;
}

export interface SelectRow {
	type: 'select';
	key: SettingKey;
	label: MessageKey;
	text: MessageKey;
	options: SelectOptionRow[];
}

/** 数値の項目。よく使う値の選択肢に加えて、「カスタム」で範囲内の好きな数値を入力できる */
export interface NumberRow {
	type: 'number';
	key: NumberSettingKey;
	label: MessageKey;
	text: MessageKey;
	/** 選択肢（値と表示名）。値 0 を含められる（「オフ」など。カスタムでは入力できない） */
	presets: { value: number; label: MessageKey }[];
	/** カスタム入力欄の横に出す単位 */
	unit: MessageKey;
}

export type SettingRow = ToggleRow | SelectRow | NumberRow;

/** 設計書 7章の選択肢と一致させる（数値の項目は、選択肢 + カスタム） */
export const SETTING_ROWS: Record<SettingsTab, SettingRow[]> = {
	general: [
		{
			type: 'toggle',
			key: 'autoSaveAfterRestore',
			label: 'settings.auto_save_after_restore',
			text: 'settings.auto_save_after_restore_text'
		},
		{
			type: 'number',
			key: 'largeFileWarnMb',
			label: 'settings.large_file_warn',
			text: 'settings.large_file_warn_text',
			presets: [
				{ value: 25, label: 'settings.opt_mb_25' },
				{ value: 50, label: 'settings.opt_mb_50' },
				{ value: 100, label: 'settings.opt_mb_100' }
			],
			unit: 'settings.unit_mb'
		},
		{
			type: 'select',
			key: 'showSnapshotsInTimeline',
			label: 'settings.show_snapshots',
			text: 'settings.show_snapshots_text',
			options: [
				{ value: 'collapsed', label: 'settings.opt_snap_collapsed' },
				{ value: 'shown', label: 'settings.opt_snap_shown' },
				{ value: 'hidden', label: 'settings.opt_snap_hidden' }
			]
		}
	],
	fetch: [
		{
			type: 'toggle',
			key: 'pullOnStartup',
			label: 'settings.auto_fetch_on_launch',
			text: 'settings.auto_fetch_on_launch_text'
		},
		{
			type: 'number',
			key: 'pullIntervalMinutes',
			label: 'settings.pull_interval',
			text: 'settings.pull_interval_text',
			presets: [
				{ value: 0, label: 'settings.opt_off' },
				{ value: 5, label: 'settings.opt_interval_5' },
				{ value: 15, label: 'settings.opt_interval_15' },
				{ value: 60, label: 'settings.opt_interval_60' }
			],
			unit: 'settings.unit_minutes'
		},
		{
			type: 'toggle',
			key: 'saveBeforePull',
			label: 'settings.auto_save_before_fetch',
			text: 'settings.auto_save_before_fetch_text'
		}
	],
	push: [
		{
			type: 'toggle',
			key: 'autoPushAfterSave',
			label: 'settings.auto_upload_on_save',
			text: 'settings.auto_upload_on_save_text'
		}
	],
	auto_save: [
		{
			type: 'toggle',
			key: 'autoSnapshotEnabled',
			label: 'settings.auto_snapshot',
			text: 'settings.auto_snapshot_text'
		},
		{
			type: 'number',
			key: 'autoSnapshotDelaySecs',
			label: 'settings.auto_snapshot_delay',
			text: 'settings.auto_snapshot_delay_text',
			presets: [
				{ value: 30, label: 'settings.opt_delay_30' },
				{ value: 120, label: 'settings.opt_delay_120' },
				{ value: 600, label: 'settings.opt_delay_600' }
			],
			unit: 'settings.unit_seconds'
		},
		{
			type: 'number',
			key: 'snapshotRetentionDays',
			label: 'settings.snapshot_retention',
			text: 'settings.snapshot_retention_text',
			presets: [
				{ value: 30, label: 'settings.opt_keep_30' },
				{ value: 90, label: 'settings.opt_keep_90' },
				{ value: 365, label: 'settings.opt_keep_365' }
			],
			unit: 'settings.unit_days'
		}
	]
};

export const SETTINGS_TABS = Object.keys(SETTING_ROWS) as SettingsTab[];

export function isSettingsTab(id: string): id is SettingsTab {
	return id in SETTING_ROWS;
}

export interface SettingConfirmation {
	title: MessageKey;
	text: MessageKey;
}

/**
 * 変更の影響が大きいときだけ、確認ダイアログの文言を返す。
 * 自動保存・自動アップロードをオフにする、自動保存を残す期間を短くする場合が対象。
 */
export function confirmationFor(
	key: SettingKey,
	current: AppSettings,
	next: SettingValue
): SettingConfirmation | null {
	if (key === 'autoPushAfterSave' && current.autoPushAfterSave && next === false) {
		return {
			title: 'settings.confirm_auto_push_off_title',
			text: 'settings.confirm_auto_push_off_text'
		};
	}
	if (key === 'autoSnapshotEnabled' && current.autoSnapshotEnabled && next === false) {
		return {
			title: 'settings.confirm_auto_snapshot_off_title',
			text: 'settings.confirm_auto_snapshot_off_text'
		};
	}
	if (
		key === 'snapshotRetentionDays' &&
		typeof next === 'number' &&
		next < current.snapshotRetentionDays
	) {
		return {
			title: 'settings.confirm_retention_title',
			text: 'settings.confirm_retention_text'
		};
	}
	return null;
}

/** 選択肢の値（文字列で受け取る）から、元の型の値を引く */
export function optionValue(row: SelectRow, picked: string): number | string | undefined {
	return row.options.find((option) => String(option.value) === picked)?.value;
}

/** 設定が上書きされているか（プロジェクト別の設定を表示しているとき） */
export function isOverridden(overridden: SettingKey[], key: SettingKey): boolean {
	return overridden.includes(key);
}

/** 画面に先に反映する更新後の設定。プロジェクト別のときは変更した項目を上書き済みにする */
export function applyPatch(
	view: SettingsView,
	patch: Partial<AppSettings>,
	isProject: boolean
): SettingsView {
	const changed = Object.keys(patch) as SettingKey[];
	return {
		settings: { ...view.settings, ...patch },
		overridden: isProject ? [...new Set([...view.overridden, ...changed])] : view.overridden
	};
}

/** 数値の項目の「カスタム」を表す、選択欄の値 */
export const CUSTOM_OPTION = 'custom';

/** 現在の値が選択肢のどれかと一致するか */
export function matchesPreset(row: NumberRow, value: number): boolean {
	return row.presets.some((preset) => preset.value === value);
}

/**
 * 数値の項目で、選択欄に出す値。カスタム入力中、または現在の値が選択肢に無いときは「カスタム」。
 */
export function numberSelectValue(row: NumberRow, value: number, customMode: boolean): string {
	return customMode || !matchesPreset(row, value) ? CUSTOM_OPTION : String(value);
}
