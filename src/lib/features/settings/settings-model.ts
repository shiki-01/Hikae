import type { AppSettings, SettingKey, SettingsView } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

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

export type SettingRow = ToggleRow | SelectRow;

/** 設計書 7章の選択肢と一致させる */
export const SETTING_ROWS: Record<SettingsTab, SettingRow[]> = {
	general: [
		{
			type: 'select',
			key: 'largeFileWarnMb',
			label: 'settings.large_file_warn',
			text: 'settings.large_file_warn_text',
			options: [
				{ value: 25, label: 'settings.opt_mb_25' },
				{ value: 50, label: 'settings.opt_mb_50' },
				{ value: 100, label: 'settings.opt_mb_100' }
			]
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
			type: 'select',
			key: 'pullIntervalMinutes',
			label: 'settings.pull_interval',
			text: 'settings.pull_interval_text',
			options: [
				{ value: 0, label: 'settings.opt_off' },
				{ value: 5, label: 'settings.opt_interval_5' },
				{ value: 15, label: 'settings.opt_interval_15' },
				{ value: 60, label: 'settings.opt_interval_60' }
			]
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
			type: 'select',
			key: 'snapshotRetentionDays',
			label: 'settings.snapshot_retention',
			text: 'settings.snapshot_retention_text',
			options: [
				{ value: 30, label: 'settings.opt_keep_30' },
				{ value: 90, label: 'settings.opt_keep_90' },
				{ value: 365, label: 'settings.opt_keep_365' }
			]
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
