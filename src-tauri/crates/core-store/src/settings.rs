// 設定（設計書 7章・7.1）。
//
// - 全体の設定は `settings`、プロジェクト別の上書きは `project_settings` に、
//   キー名と JSON 値の組として保存する。保存されていないキーは既定値になる。
// - 値は保存前に検証する（選択肢にない値は拒否）。保存済みの値が壊れていた場合は
//   そのキーだけ既定値に戻して読む（設定の破損でアプリを使えなくしない）。
// - Phase 2 以降の項目（AI、自動保存、拡張機能など）は、ここでは保存するだけで挙動は持たない。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// ぶつかり発生時の動作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum ConflictMode {
    /// すぐ解消画面を開く
    ShowDialog,
    /// 通知だけ出す
    NotifyOnly,
}

/// メモの自動提案の方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum MemoSuggestion {
    /// ルールのみ
    Rules,
    /// AI を使う
    Ai,
}

/// 自動保存をタイムラインに出すか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum TimelineSnapshots {
    Collapsed,
    Shown,
    Hidden,
}

/// 「開く」の既定動作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum OpenAction {
    DefaultApp,
    VsCode,
}

/// AI の実行方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum AiRunner {
    Builtin,
    Ollama,
}

/// AI に渡す範囲
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum AiScope {
    FileNames,
    TextDiff,
    WithImages,
}

/// AI モデルの指定方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum AiModelSource {
    Catalog,
    CustomGguf,
}

/// git 実行ファイルの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitExecutable {
    Bundled,
    System,
}

/// 新規リポジトリの既定の公開範囲
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum DefaultVisibility {
    Private,
    Public,
}

/// 用語の表示
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum TermDisplay {
    /// 平易な表現のみ
    Plain,
    /// Git 用語を併記
    WithGit,
}

/// 設定項目と既定値から、全体設定の構造体（`AppSettings`）と更新用の部分指定（`SettingsPatch`）を作る。
macro_rules! define_settings {
    ($( $(#[$meta:meta])* $name:ident : $ty:ty = $default:expr ),* $(,)?) => {
        /// 全体設定（またはプロジェクト別の上書きを反映した、実際に使う設定）
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        pub struct AppSettings {
            $( $(#[$meta])* pub $name: $ty, )*
        }

        impl Default for AppSettings {
            fn default() -> Self {
                AppSettings { $( $name: $default, )* }
            }
        }

        /// 設定の更新内容。指定した項目だけを書き換える。
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        #[serde(default)]
        pub struct SettingsPatch {
            $(
                $(#[$meta])*
                #[serde(skip_serializing_if = "Option::is_none")]
                pub $name: Option<$ty>,
            )*
        }
    };
}

define_settings! {
    // ----- 初回設定 -----
    /// 初回設定（ログインと保存先の案内）を完了したか
    onboarded: bool = false,

    // ----- 取り込み（プロジェクト別に上書きできる） -----
    /// 起動時に取り込む
    pull_on_startup: bool = true,
    /// 定期的に取り込む間隔（分）。0 はオフ。選択肢は 0 / 5 / 15 / 60
    pull_interval_minutes: u32 = 15,
    /// 取り込む前に未保存の変更を保存する（false は「確認する」）
    save_before_pull: bool = true,
    /// ぶつかり発生時の動作
    conflict_mode: ConflictMode = ConflictMode::NotifyOnly,

    // ----- アップロード（プロジェクト別に上書きできる） -----
    /// 保存時に自動アップロードする
    auto_push_after_save: bool = true,
    /// アップロード待ちの通知間隔（時間）。0 はオフ。選択肢は 0 / 1 / 24
    push_reminder_hours: u32 = 24,

    // ----- 自動保存（プロジェクト別に上書きできる。Phase 2） -----
    /// ファイル監視による自動保存
    auto_snapshot_enabled: bool = true,
    /// 最後の変更からこの秒数だけ静止してから記録する。選択肢は 30 / 120 / 600
    auto_snapshot_delay_secs: u32 = 120,
    /// 自動保存の保持期間（日）。選択肢は 30 / 90 / 365
    snapshot_retention_days: u32 = 90,

    // ----- 保存・表示・外部アプリ（プロジェクト別に上書きできる） -----
    /// メモの自動提案の方式
    memo_suggestion: MemoSuggestion = MemoSuggestion::Rules,
    /// 元に戻した後に自動で保存する
    auto_save_after_restore: bool = true,
    /// 大きいファイルの警告閾値（MB）。選択肢は 25 / 50 / 100
    large_file_warn_mb: u32 = 50,
    /// 自動保存をタイムラインに表示するか
    show_snapshots_in_timeline: TimelineSnapshots = TimelineSnapshots::Collapsed,
    /// 「開く」の既定動作
    open_action: OpenAction = OpenAction::DefaultApp,

    // ----- AI（アプリ全体。Phase 2） -----
    /// 実行方式
    ai_runner: AiRunner = AiRunner::Builtin,
    /// 使用モデルの識別子。空は未インストール
    ai_model: String = String::new(),
    /// AI に渡す範囲
    ai_scope: AiScope = AiScope::TextDiff,

    // ----- 詳細（アプリ全体） -----
    /// git 実行ファイル
    git_executable: GitExecutable = GitExecutable::Bundled,
    /// 技術情報をエラーに表示する
    show_technical_info: bool = false,
    /// 新規リポジトリの既定の公開範囲
    default_visibility: DefaultVisibility = DefaultVisibility::Private,
    /// 自動保存の容量上限（プロジェクトサイズの倍数）。0 は上限なし。選択肢は 0 / 1 / 2 / 5。Phase 2
    snapshot_size_cap_x: u32 = 2,
    /// AI モデルの指定方法。Phase 2
    ai_model_source: AiModelSource = AiModelSource::Catalog,
    /// 任意の GGUF ファイルのパス（`ai_model_source` が `custom-gguf` のとき）。Phase 2
    ai_custom_gguf_path: String = String::new(),
    /// 作業コピー機能を表示する。Phase 3
    work_copy_enabled: bool = false,
    /// 拡張機能の開発者モード（署名なしの手動導入）。Phase 3
    extension_dev_mode: bool = false,
    /// 拡張機能の追加索引の URL。空は公式のみ。Phase 4
    extension_index_urls: Vec<String> = Vec::new(),
    /// Git LFS を有効にする。Phase 4
    git_lfs: bool = false,
    /// 用語の表示
    term_display: TermDisplay = TermDisplay::Plain,
}

/// プロジェクト別に上書きできる項目のキー名（それ以外はアプリ全体でのみ設定する）
pub const PROJECT_OVERRIDABLE_KEYS: &[&str] = &[
    "pull_on_startup",
    "pull_interval_minutes",
    "save_before_pull",
    "conflict_mode",
    "auto_push_after_save",
    "push_reminder_hours",
    "auto_snapshot_enabled",
    "auto_snapshot_delay_secs",
    "snapshot_retention_days",
    "memo_suggestion",
    "auto_save_after_restore",
    "large_file_warn_mb",
    "show_snapshots_in_timeline",
    "open_action",
];

/// 文字列項目の最大長
const MAX_TEXT_LEN: usize = 1024;
/// 追加索引 URL の最大件数
const MAX_INDEX_URLS: usize = 20;

fn in_set(name: &str, value: u32, allowed: &[u32]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{name} の値 {value} は選べません（選択肢: {allowed:?}）"
        ))
    }
}

fn plain_text(name: &str, value: &str) -> Result<(), String> {
    if value.chars().count() > MAX_TEXT_LEN || value.chars().any(char::is_control) {
        Err(format!(
            "{name} に使えない文字、または長すぎる値が含まれています"
        ))
    } else {
        Ok(())
    }
}

impl AppSettings {
    /// 値が選択肢の範囲にあるか検証する。違反があれば理由（技術情報）を返す。
    pub fn validate(&self) -> Result<(), String> {
        in_set(
            "pull_interval_minutes",
            self.pull_interval_minutes,
            &[0, 5, 15, 60],
        )?;
        in_set("push_reminder_hours", self.push_reminder_hours, &[0, 1, 24])?;
        in_set(
            "auto_snapshot_delay_secs",
            self.auto_snapshot_delay_secs,
            &[30, 120, 600],
        )?;
        in_set(
            "snapshot_retention_days",
            self.snapshot_retention_days,
            &[30, 90, 365],
        )?;
        in_set(
            "large_file_warn_mb",
            self.large_file_warn_mb,
            &[25, 50, 100],
        )?;
        in_set(
            "snapshot_size_cap_x",
            self.snapshot_size_cap_x,
            &[0, 1, 2, 5],
        )?;
        plain_text("ai_model", &self.ai_model)?;
        plain_text("ai_custom_gguf_path", &self.ai_custom_gguf_path)?;
        if self.extension_index_urls.len() > MAX_INDEX_URLS {
            return Err("extension_index_urls が多すぎます".to_string());
        }
        for url in &self.extension_index_urls {
            plain_text("extension_index_urls", url)?;
            if !url.starts_with("https://") || url.contains(char::is_whitespace) {
                return Err(
                    "extension_index_urls には https:// の URL だけ指定できます".to_string()
                );
            }
        }
        Ok(())
    }

    /// 更新内容を反映した設定を返す。
    pub fn applied(&self, patch: &SettingsPatch) -> AppSettings {
        let mut base = to_map(self);
        for (key, value) in to_map(patch) {
            base.insert(key, value);
        }
        // 型が合わないことはない（同じ項目の型から作った値）。万一失敗しても元の設定を保つ
        serde_json::from_value(Value::Object(base)).unwrap_or_else(|_| self.clone())
    }
}

impl SettingsPatch {
    /// 指定されている項目のキー名
    pub fn keys(&self) -> Vec<String> {
        to_map(self).keys().cloned().collect()
    }

    /// 何も指定されていないか
    pub fn is_empty(&self) -> bool {
        to_map(self).is_empty()
    }
}

/// 構造体を JSON のオブジェクト（項目名 → 値）にする。
pub(crate) fn to_map<T: Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// 保存されていた（キー, JSON 文字列）の組から、有効なものだけを集める。
/// 未知のキー、JSON として読めない値、型や選択肢に合わない値は無視する（既定値のままになる）。
pub(crate) fn valid_stored_entries(
    stored: impl IntoIterator<Item = (String, String)>,
    only_keys: Option<&[&str]>,
) -> Map<String, Value> {
    let defaults = to_map(&AppSettings::default());
    let mut accepted = Map::new();
    for (key, raw) in stored {
        if !defaults.contains_key(&key) {
            continue;
        }
        if only_keys.is_some_and(|keys| !keys.contains(&key.as_str())) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let mut trial = defaults.clone();
        for (k, v) in &accepted {
            trial.insert(k.clone(), v.clone());
        }
        trial.insert(key.clone(), value.clone());
        match serde_json::from_value::<AppSettings>(Value::Object(trial)) {
            Ok(settings) if settings.validate().is_ok() => {
                accepted.insert(key, value);
            }
            _ => {}
        }
    }
    accepted
}

/// 有効な保存値から全体設定を作る。
pub(crate) fn settings_from_entries(entries: Map<String, Value>) -> AppSettings {
    let mut base = to_map(&AppSettings::default());
    for (k, v) in entries {
        base.insert(k, v);
    }
    serde_json::from_value(Value::Object(base)).unwrap_or_default()
}

/// 有効な保存値から上書きの部分指定を作る。
pub(crate) fn patch_from_entries(entries: Map<String, Value>) -> SettingsPatch {
    serde_json::from_value(Value::Object(entries)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_chapter_7() {
        let d = AppSettings::default();
        assert!(d.pull_on_startup);
        assert_eq!(d.pull_interval_minutes, 15);
        assert!(d.save_before_pull);
        // 設計書 7章: ぶつかり発生時の既定は「通知だけ出す」
        assert_eq!(d.conflict_mode, ConflictMode::NotifyOnly);
        assert!(d.auto_push_after_save);
        assert_eq!(d.push_reminder_hours, 24);
        assert!(d.auto_snapshot_enabled);
        assert_eq!(d.auto_snapshot_delay_secs, 120);
        assert_eq!(d.snapshot_retention_days, 90);
        assert_eq!(d.memo_suggestion, MemoSuggestion::Rules);
        assert!(d.auto_save_after_restore);
        assert_eq!(d.large_file_warn_mb, 50);
        assert_eq!(d.show_snapshots_in_timeline, TimelineSnapshots::Collapsed);
        assert_eq!(d.open_action, OpenAction::DefaultApp);
        assert_eq!(d.ai_runner, AiRunner::Builtin);
        assert_eq!(d.ai_model, "");
        assert_eq!(d.ai_scope, AiScope::TextDiff);
        assert_eq!(d.git_executable, GitExecutable::Bundled);
        assert!(!d.show_technical_info);
        // 7.1
        assert_eq!(d.default_visibility, DefaultVisibility::Private);
        assert_eq!(d.snapshot_size_cap_x, 2);
        assert_eq!(d.ai_model_source, AiModelSource::Catalog);
        assert!(!d.work_copy_enabled);
        assert!(!d.extension_dev_mode);
        assert!(d.extension_index_urls.is_empty());
        assert!(!d.git_lfs);
        assert_eq!(d.term_display, TermDisplay::Plain);
        assert!(!d.onboarded);
        assert!(d.validate().is_ok());
    }

    #[test]
    fn patch_serializes_only_given_fields() {
        let patch = SettingsPatch {
            pull_interval_minutes: Some(5),
            conflict_mode: Some(ConflictMode::ShowDialog),
            ..SettingsPatch::default()
        };
        let mut keys = patch.keys();
        keys.sort();
        assert_eq!(keys, vec!["conflict_mode", "pull_interval_minutes"]);
        assert!(SettingsPatch::default().is_empty());

        let applied = AppSettings::default().applied(&patch);
        assert_eq!(applied.pull_interval_minutes, 5);
        assert_eq!(applied.conflict_mode, ConflictMode::ShowDialog);
        // 指定していない項目は変わらない
        assert!(applied.auto_push_after_save);
    }

    #[test]
    fn enum_values_use_kebab_case_names() {
        let json = serde_json::to_value(AppSettings::default()).expect("json");
        assert_eq!(json["conflict_mode"], "notify-only");
        assert_eq!(json["show_snapshots_in_timeline"], "collapsed");
        assert_eq!(json["open_action"], "default-app");
    }

    #[test]
    fn validate_rejects_values_outside_the_choices() {
        let bad = |f: fn(&mut AppSettings)| {
            let mut s = AppSettings::default();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(|s| s.pull_interval_minutes = 7));
        assert!(bad(|s| s.push_reminder_hours = 2));
        assert!(bad(|s| s.auto_snapshot_delay_secs = 1));
        assert!(bad(|s| s.snapshot_retention_days = 10));
        assert!(bad(|s| s.large_file_warn_mb = 10));
        assert!(bad(|s| s.snapshot_size_cap_x = 3));
        assert!(bad(|s| s.ai_model = "a\nb".to_string()));
        assert!(bad(
            |s| s.extension_index_urls = vec!["http://x.example".to_string()]
        ));
        assert!(bad(
            |s| s.extension_index_urls = vec!["https://x y".to_string()]
        ));

        let ok = AppSettings {
            pull_interval_minutes: 0,
            extension_index_urls: vec!["https://index.example/list.json".to_string()],
            ..AppSettings::default()
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn stored_entries_skip_unknown_broken_and_invalid_values() {
        let entries = valid_stored_entries(
            [
                ("pull_interval_minutes", "5"),
                ("auto_push_after_save", "false"),
                ("not_a_setting", "1"),
                ("conflict_mode", "\"explode\""),
                ("large_file_warn_mb", "77"),
                ("push_reminder_hours", "not json"),
                ("git_lfs", "\"yes\""),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string())),
            None,
        );
        let s = settings_from_entries(entries);
        assert_eq!(s.pull_interval_minutes, 5);
        assert!(!s.auto_push_after_save);
        // 壊れた値・選択肢外の値は既定値のまま
        assert_eq!(s.conflict_mode, ConflictMode::NotifyOnly);
        assert_eq!(s.large_file_warn_mb, 50);
        assert_eq!(s.push_reminder_hours, 24);
        assert!(!s.git_lfs);
    }

    #[test]
    fn stored_entries_can_be_limited_to_overridable_keys() {
        let entries = valid_stored_entries(
            [("pull_interval_minutes", "5"), ("git_lfs", "true")]
                .map(|(k, v)| (k.to_string(), v.to_string())),
            Some(PROJECT_OVERRIDABLE_KEYS),
        );
        let patch = patch_from_entries(entries);
        assert_eq!(patch.pull_interval_minutes, Some(5));
        assert_eq!(patch.git_lfs, None);
    }

    #[test]
    fn overridable_keys_are_real_settings() {
        let all = to_map(&AppSettings::default());
        for key in PROJECT_OVERRIDABLE_KEYS {
            assert!(all.contains_key(*key), "{key}");
        }
    }
}
