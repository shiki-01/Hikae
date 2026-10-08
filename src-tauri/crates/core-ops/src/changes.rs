// 作業フォルダの変更ファイル一覧（未保存の変更）。
// `git status` の機械可読出力を core-git のパーサで解析する。読み取りのみで、作業フォルダ・
// インデックスは変更しない（status は GIT_OPTIONAL_LOCKS=0 付きで実行される）。

use crate::models::OpsError;
use core_git::{GitRunner, StatusCode, StatusKind};
use std::path::Path;

/// 変更の種類（画面の「追加・変更・削除・名前変更」に対応する）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangedKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// 未保存の変更 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// プロジェクト内の相対パス（`/` 区切り）
    pub path: String,
    pub kind: ChangedKind,
    /// 変更のぶつかり中のファイルか
    pub conflicted: bool,
    /// 名前変更の場合の元のパス
    pub original_path: Option<String>,
    /// 一度も保存されておらず、インデックスにも入っていない新規ファイル（`git status` の `?`）。
    /// 「元に戻す（作成しない）」の対象になれるのはこれだけ
    pub untracked: bool,
}

/// 未保存の変更を 1 ファイルずつ列挙する。
/// 未追跡ファイルはフォルダ単位にまとめず（`-uall`）、保存対象外（.gitignore）のファイルは含めない。
pub(crate) fn list_changes(runner: &GitRunner, repo: &Path) -> Result<Vec<ChangedFile>, OpsError> {
    let out = runner.run_ok(repo, &["status", "--porcelain=v2", "-z", "-uall"])?;
    let status =
        core_git::parse_status_v2(&out.stdout).map_err(|e| OpsError::Unexpected(e.to_string()))?;
    Ok(status
        .entries
        .iter()
        .filter_map(|entry| {
            let (kind, conflicted, original_path) = match &entry.kind {
                StatusKind::Ignored => return None,
                StatusKind::Untracked => (ChangedKind::Added, false, None),
                StatusKind::Unmerged { .. } => (ChangedKind::Modified, true, None),
                StatusKind::Rename {
                    worktree,
                    copy,
                    original_path,
                    ..
                } => {
                    if *worktree == StatusCode::Deleted {
                        (ChangedKind::Deleted, false, None)
                    } else if *copy {
                        (ChangedKind::Added, false, None)
                    } else {
                        (ChangedKind::Renamed, false, Some(original_path.clone()))
                    }
                }
                StatusKind::Change { index, worktree } => {
                    let kind = if *worktree == StatusCode::Deleted
                        || (*index == StatusCode::Deleted && *worktree == StatusCode::Unmodified)
                    {
                        ChangedKind::Deleted
                    } else if *index == StatusCode::Added || *worktree == StatusCode::Added {
                        ChangedKind::Added
                    } else {
                        ChangedKind::Modified
                    };
                    (kind, false, None)
                }
            };
            Some(ChangedFile {
                path: entry.path.clone(),
                kind,
                conflicted,
                original_path,
                untracked: matches!(entry.kind, StatusKind::Untracked),
            })
        })
        .collect())
}
