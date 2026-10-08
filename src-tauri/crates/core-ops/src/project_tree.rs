// プロジェクトフォルダ全体のファイル一覧（設計書 3.3 S2「すべてのファイル」）。
//
// 保存対象のファイル（追跡済み + 未追跡。`.gitignore` の対象と `.git` は除く）を
// `git ls-files -z -c -o --exclude-standard` で一覧にし、各ファイルに変更の種類を付ける。
// 大きなフォルダで重くならないよう件数に上限を設け、超えたら打ち切って `truncated` を立てる。
// 変更のあるファイルは、上限を超えても先に含める。読み取りのみで、作業フォルダ・インデックスは
// 変更しない。

use crate::changes::{list_changes, ChangedKind};
use crate::models::OpsError;
use core_git::GitRunner;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// 一覧に含めるファイル数の上限
pub const PROJECT_TREE_MAX_ENTRIES: usize = 10_000;

/// 一覧の 1 ファイル
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTreeEntry {
    /// プロジェクト内の相対パス（`/` 区切り）
    pub path: String,
    /// ファイルサイズ（バイト）。作業フォルダに無い（削除された）ときは None
    pub size: Option<u64>,
    /// 最終更新（Unix 秒）。取得できなければ None
    pub modified_unix: Option<i64>,
    /// 未保存の変更の種類。変更が無ければ None
    pub change: Option<ChangedKind>,
    /// 変更のぶつかり中のファイルか
    pub conflicted: bool,
}

/// フォルダ全体の一覧
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTree {
    /// パス順のファイル一覧
    pub entries: Vec<ProjectTreeEntry>,
    /// 上限を超えたため、一部のみを含む
    pub truncated: bool,
}

pub(crate) fn list_project_tree(
    runner: &GitRunner,
    repo: &Path,
    max_entries: usize,
) -> Result<ProjectTree, OpsError> {
    let changes = list_changes(runner, repo)?;
    let out = runner.run_ok(repo, &["ls-files", "-z", "-c", "-o", "--exclude-standard"])?;

    // 重複（ぶつかり中のファイルは複数のステージで出る）を除き、パス順にそろえる
    let mut listed: BTreeSet<String> = BTreeSet::new();
    for raw in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(raw).into_owned();
        if is_listable(&path) {
            listed.insert(path);
        }
    }

    // 変更のあるファイルを先に確保し、残りの枠をそのほかのファイルで埋める。
    // 削除されたファイルはインデックスから外れている場合があり、`ls-files` に出ないことがある
    let change_by_path: HashMap<&str, (ChangedKind, bool)> = changes
        .iter()
        .map(|c| (c.path.as_str(), (c.kind, c.conflicted)))
        .collect();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    for change in &changes {
        if is_listable(&change.path) {
            chosen.insert(change.path.clone());
        }
    }
    let mut truncated = chosen.len() > max_entries;
    if truncated {
        chosen = chosen.into_iter().take(max_entries).collect();
    }
    for path in &listed {
        if chosen.contains(path) {
            continue;
        }
        if chosen.len() >= max_entries {
            truncated = true;
            break;
        }
        chosen.insert(path.clone());
    }

    let mut entries = Vec::with_capacity(chosen.len());
    for path in chosen {
        let (size, modified_unix) = match std::fs::symlink_metadata(repo.join(&path)) {
            // サブモジュールなど、フォルダとして存在するものはファイルとして扱わない
            Ok(meta) if meta.is_dir() => continue,
            Ok(meta) => (Some(meta.len()), modified_unix(&meta)),
            Err(_) => (None, None),
        };
        let (change, conflicted) = match change_by_path.get(path.as_str()) {
            Some((kind, conflicted)) => (Some(*kind), *conflicted),
            None => (None, false),
        };
        entries.push(ProjectTreeEntry {
            path,
            size,
            modified_unix,
            change,
            conflicted,
        });
    }
    Ok(ProjectTree { entries, truncated })
}

/// 一覧に出してよいパスか。入れ子のリポジトリ（末尾が `/`）と `.git` の配下は除く。
fn is_listable(path: &str) -> bool {
    !path.ends_with('/')
        && !path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
}

fn modified_unix(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_repositories_and_git_internals_are_not_listed() {
        assert!(is_listable("a/b.txt"));
        assert!(is_listable("資料/第3章.docx"));
        assert!(!is_listable("nested/"));
        assert!(!is_listable(".git/config"));
        assert!(!is_listable("sub/.GIT/hooks"));
    }
}
