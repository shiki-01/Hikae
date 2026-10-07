// 操作ジャーナル（設計書 6.3）。状態変更操作の実行結果を SQLite に追記する。
//
// 認証情報を残さないため、記録する文字列は `redact` を通して切り詰める。

use serde::{Deserialize, Serialize};

/// 操作の結果
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum JournalOutcome {
    Success,
    Failure,
}

impl JournalOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JournalOutcome::Success => "success",
            JournalOutcome::Failure => "failure",
        }
    }

    pub(crate) fn from_db(s: &str) -> Self {
        if s == "success" {
            JournalOutcome::Success
        } else {
            JournalOutcome::Failure
        }
    }
}

/// 操作の起動元
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum JournalTrigger {
    /// ユーザーの操作
    Manual,
    /// スケジューラによる自動実行
    Auto,
}

impl JournalTrigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JournalTrigger::Manual => "manual",
            JournalTrigger::Auto => "auto",
        }
    }

    pub(crate) fn from_db(s: &str) -> Self {
        if s == "auto" {
            JournalTrigger::Auto
        } else {
            JournalTrigger::Manual
        }
    }
}

/// 追記する 1 件
#[derive(Debug, Clone)]
pub struct NewJournalEntry {
    pub project_id: String,
    /// `save` / `restore` / `pull` / `push` / `resolve` など
    pub operation: String,
    pub trigger: JournalTrigger,
    /// RFC3339
    pub started_at: String,
    /// RFC3339
    pub finished_at: String,
    pub outcome: JournalOutcome,
    /// 結果の要約（`merged` など）または失敗の種類。エラー本文は入れない
    pub detail: Option<String>,
    /// 操作が作った自動保存（スナップショット）の ref
    pub snapshot_ref: Option<String>,
    /// 操作が作ったバックアップ ref
    pub backup_ref: Option<String>,
    /// 対象（元に戻す先のコミットなど）
    pub target: Option<String>,
}

/// 記録済みの 1 件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub id: i64,
    pub project_id: String,
    pub operation: String,
    pub trigger: JournalTrigger,
    pub started_at: String,
    pub finished_at: String,
    pub outcome: JournalOutcome,
    pub detail: Option<String>,
    pub snapshot_ref: Option<String>,
    pub backup_ref: Option<String>,
    pub target: Option<String>,
}

/// 現在時刻（RFC3339）。ジャーナルの日時に使う。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 記録する文字列の最大長
const MAX_TEXT_CHARS: usize = 200;

/// 認証情報になりうる語頭（GitHub のトークン形式）
const TOKEN_PREFIXES: &[&str] = &["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];

/// 認証情報らしき部分を伏せ、長さを制限する。
/// - `scheme://user:pass@host` のユーザー情報部分
/// - GitHub のトークン形式の語
/// - `Authorization` / `Bearer` / `token=` / `password=` に続く語
pub fn redact(text: &str) -> String {
    let mut out = Vec::new();
    // 直後に続く語を伏せる残り個数
    let mut hide_count = 0;
    for word in text.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let mut w = word.to_string();

        if hide_count > 0 {
            hide_count -= 1;
            out.push("***".to_string());
            continue;
        }
        if lower == "authorization:" || lower == "authorization" {
            // `Authorization: Bearer <token>` / `Basic <credentials>` の 2 語を伏せる
            hide_count = 2;
            out.push(w);
            continue;
        }
        if lower == "bearer" {
            hide_count = 1;
            out.push(w);
            continue;
        }
        if TOKEN_PREFIXES.iter().any(|p| lower.contains(p)) {
            out.push("***".to_string());
            continue;
        }
        if let Some((key, _)) = w.split_once('=') {
            let key = key.to_ascii_lowercase();
            if ["token", "password", "passwd", "secret", "access_token"]
                .iter()
                .any(|k| key.ends_with(k))
            {
                out.push(format!("{}=***", &w[..key.len()]));
                continue;
            }
        }
        if let Some(scheme_end) = w.find("://") {
            let rest = &w[scheme_end + 3..];
            let authority_end = rest.find('/').unwrap_or(rest.len());
            if let Some(at) = rest[..authority_end].rfind('@') {
                w = format!("{}://***@{}", &w[..scheme_end], &rest[at + 1..]);
            }
        }
        out.push(w);
    }
    let joined = out.join(" ");
    joined.chars().take(MAX_TEXT_CHARS).collect()
}
