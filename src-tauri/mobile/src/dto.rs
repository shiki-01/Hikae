// 画面とやり取りする型（camelCase。`src/lib/api/mobile-types.ts` と対応）。
// core-remote の型から変換する。トークンは含めない。

use core_github::User;
use core_remote as cr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileAccount {
    pub login: String,
    pub avatar_url: Option<String>,
}

impl From<&User> for MobileAccount {
    fn from(u: &User) -> Self {
        Self {
            login: u.login.clone(),
            avatar_url: u.avatar_url.clone(),
        }
    }
}

/// ログイン手順（device_code は含まない）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum LoginOutcome {
    Succeeded,
    Denied,
    Expired,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileRepo {
    pub owner: String,
    pub name: String,
    pub private: bool,
    pub default_branch: String,
    pub pushed_at: Option<String>,
    pub description: Option<String>,
}

impl From<cr::RepoSummary> for MobileRepo {
    fn from(r: cr::RepoSummary) -> Self {
        Self {
            owner: r.owner,
            name: r.name,
            private: r.private,
            default_branch: r.default_branch,
            pushed_at: r.pushed_at,
            description: r.description,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    /// バイト数。TS では number として扱う（25 MiB 程度までで、2^53 を超えない）
    pub size: f64,
    pub sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub branch: String,
    pub head_sha: String,
    pub truncated: bool,
    pub entries: Vec<DirEntry>,
}

impl From<cr::DirListing> for DirListing {
    fn from(l: cr::DirListing) -> Self {
        Self {
            branch: l.branch,
            head_sha: l.head_sha,
            truncated: l.truncated,
            entries: l
                .entries
                .into_iter()
                .map(|e| DirEntry {
                    name: e.name,
                    path: e.path,
                    kind: match e.kind {
                        cr::EntryKind::File => EntryKind::File,
                        cr::EntryKind::Dir => EntryKind::Dir,
                    },
                    size: e.size as f64,
                    sha: e.sha,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileVersion {
    pub sha: String,
    pub message: String,
    pub date: String,
    pub author: String,
}

impl From<cr::VersionInfo> for MobileVersion {
    fn from(v: cr::VersionInfo) -> Self {
        Self {
            sha: v.sha,
            message: v.message,
            date: v.date,
            author: v.author,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DocSection {
    pub title: Option<String>,
    pub paragraphs: Vec<String>,
    pub rows: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DocFormat {
    Docx,
    Xlsx,
    Pptx,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MobilePreview {
    Text { text: String, truncated: bool },
    Image { mime: String, content_base64: String },
    Pdf { content_base64: String },
    Document { format: DocFormat, sections: Vec<DocSection> },
    Unsupported,
    TooLarge,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileDownload {
    pub file_name: String,
    pub mime: String,
    pub content_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemInput {
    pub target_dir: String,
    pub name: String,
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItem {
    pub target_path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UploadPlan {
    pub branch: String,
    pub items: Vec<PlannedItem>,
}

impl From<cr::UploadPlan> for UploadPlan {
    fn from(p: cr::UploadPlan) -> Self {
        Self {
            branch: p.branch,
            items: p
                .items
                .into_iter()
                .map(|i| PlannedItem {
                    target_path: i.target_path,
                    exists: i.exists,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum OnExists {
    Replace,
    KeepBoth,
    Skip,
}

impl From<OnExists> for cr::OnExists {
    fn from(o: OnExists) -> Self {
        match o {
            OnExists::Replace => cr::OnExists::Replace,
            OnExists::KeepBoth => cr::OnExists::KeepBoth,
            OnExists::Skip => cr::OnExists::Skip,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UploadItemInput {
    pub target_dir: String,
    pub name: String,
    pub content_base64: String,
    pub on_exists: OnExists,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Renamed {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub commit_sha: String,
    pub saved: Vec<String>,
    pub skipped: Vec<String>,
    pub renamed: Vec<Renamed>,
}

impl From<cr::UploadResult> for UploadResult {
    fn from(r: cr::UploadResult) -> Self {
        Self {
            commit_sha: r.commit_sha,
            saved: r.saved,
            skipped: r.skipped,
            renamed: r
                .renamed
                .into_iter()
                .map(|x| Renamed {
                    from: x.from,
                    to: x.to,
                })
                .collect(),
        }
    }
}
