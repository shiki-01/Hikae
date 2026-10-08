// Data models for projects and configurations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// プロジェクトの登録情報。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub display_name: String,
    pub path: PathBuf,
    pub remote_url: Option<String>,
    pub owner: String, // GitHub ユーザー名または Org 名
    pub default_branch: String,
    pub last_viewed_at: String, // RFC3339 形式
    pub config: String,         // JSON 形式のプロジェクト別設定
    /// 最初の保存（親を持たない commit）の OID。保存先 URL が無いプロジェクトの付け替え照合に使う。
    /// まだ保存が無い、または古い登録では None
    pub initial_commit: Option<String>,
}

/// プロジェクト別設定（7章）。JSON で保存される。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    // 取り込み設定
    #[serde(default)]
    pub auto_pull_on_startup: Option<bool>,
    #[serde(default)]
    pub pull_interval_minutes: Option<u32>,
    #[serde(default)]
    pub auto_save_before_pull: Option<bool>,
    #[serde(default)]
    pub resolve_conflict_mode: Option<String>, // "show-dialog" or "notify-only"

    // アップロード設定
    #[serde(default)]
    pub auto_push_after_save: Option<bool>,
    #[serde(default)]
    pub push_notification_interval_hours: Option<u32>,

    // その他
    #[serde(default)]
    pub auto_save_snapshots: Option<bool>,
    #[serde(default)]
    pub snapshot_retention_days: Option<u32>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            auto_pull_on_startup: Some(true),
            pull_interval_minutes: Some(15),
            auto_save_before_pull: Some(true),
            // 設計書 7章: ぶつかり発生時の既定は「通知だけ出す」
            resolve_conflict_mode: Some("notify-only".to_string()),
            auto_push_after_save: Some(true),
            push_notification_interval_hours: Some(24),
            auto_save_snapshots: Some(true),
            snapshot_retention_days: Some(90),
        }
    }
}

impl ProjectConfig {
    /// すべて未指定（既定値を使う）の設定。`Default` は 7章以外の値も持つため別に用意する。
    pub fn unset() -> Self {
        ProjectConfig {
            auto_pull_on_startup: None,
            pull_interval_minutes: None,
            auto_save_before_pull: None,
            resolve_conflict_mode: None,
            auto_push_after_save: None,
            push_notification_interval_hours: None,
            auto_save_snapshots: None,
            snapshot_retention_days: None,
        }
    }
}

impl Project {
    /// プロジェクト別の上書き設定を読む。壊れた JSON は「上書きなし」として扱う。
    pub fn config_overrides(&self) -> ProjectConfig {
        serde_json::from_str(&self.config).unwrap_or_else(|_| ProjectConfig::unset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(config: &str) -> Project {
        Project {
            id: "p".to_string(),
            display_name: "p".to_string(),
            path: PathBuf::from("/p"),
            remote_url: None,
            owner: "o".to_string(),
            default_branch: "main".to_string(),
            last_viewed_at: String::new(),
            config: config.to_string(),
            initial_commit: None,
        }
    }

    #[test]
    fn config_overrides_reads_partial_and_broken_json() {
        let c = project("{}").config_overrides();
        assert_eq!(c.pull_interval_minutes, None);
        assert_eq!(c.auto_push_after_save, None);

        let c = project(r#"{"pull_interval_minutes":5,"auto_push_after_save":false}"#)
            .config_overrides();
        assert_eq!(c.pull_interval_minutes, Some(5));
        assert_eq!(c.auto_push_after_save, Some(false));
        assert_eq!(c.auto_pull_on_startup, None);

        assert_eq!(
            project("not json").config_overrides().pull_interval_minutes,
            None
        );
    }
}
