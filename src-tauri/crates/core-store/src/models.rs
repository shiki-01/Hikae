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
            resolve_conflict_mode: Some("show-dialog".to_string()),
            auto_push_after_save: Some(true),
            push_notification_interval_hours: Some(24),
            auto_save_snapshots: Some(true),
            snapshot_retention_days: Some(90),
        }
    }
}
