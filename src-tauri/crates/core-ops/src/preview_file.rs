// 過去の版のファイルを一時フォルダへ書き出す（設計書 4.7「過去の版を開く」）。
//
// - 内容の取得は GitRunner 経由（`ls-tree` で通常のファイルと確認し、`cat-file -p <OID>:<パス>` で読む）。
//   許可リストは変更しない
// - 書き出し先は `<preview_root>/<短縮コミット>/<相対パス>`。`preview_root` は呼び出し側が
//   `<temp>/hikae-preview/<プロジェクト ID>` として渡す。作業フォルダにもリポジトリにも書かない
// - パスは `normalize_project_path`（字句検査）を通す。`..`・絶対パス・ドライブ指定・`.git` 配下は拒否
// - 書き出したファイルには読み取り専用属性を付ける（開いたアプリでの編集が履歴に紛れ込まないように）
// - 古い一時ファイルは起動時に削除する（`cleanup_old_previews`）。削除するのはこのアプリが
//   一時フォルダに書いたものだけで、プロジェクトのファイルには触れない

use crate::models::OpsError;
use crate::restore_file::{normalize_project_path, resolve_commit, size_at};
use core_git::GitRunner;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 一時ファイルを残す期間（これを超えて更新されていないものを起動時に削除する）
pub const PREVIEW_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 書き出し先フォルダ名に使うコミットの桁数
const SHORT_COMMIT_LEN: usize = 12;

/// 指定時点の 1 ファイルを一時フォルダへ書き出し、読み取り専用にしてそのパスを返す。
/// 指定時点に通常のファイルとして存在しなければ `InvalidInput`（何も書かない）。
pub(crate) fn export_file_at(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
    relative_path: &str,
    preview_root: &Path,
) -> Result<PathBuf, OpsError> {
    let rel = normalize_project_path(relative_path)?;
    let oid = resolve_commit(runner, repo, commit)?;
    if size_at(runner, repo, &oid, &rel)?.is_none() {
        return Err(OpsError::InvalidInput(
            "file does not exist at that point".to_string(),
        ));
    }

    let spec = format!("{oid}:{rel}");
    let content = runner.run_ok(repo, &["cat-file", "-p", &spec])?.stdout;

    let short: String = oid.chars().take(SHORT_COMMIT_LEN).collect();
    let mut dest = preview_root.join(short);
    for part in rel.split('/') {
        dest.push(part);
    }
    // 成分はすべて通常の名前（`normalize_project_path` で保証済み）だが、念のため配下であることを確認する
    if !dest.starts_with(preview_root) {
        return Err(OpsError::InvalidInput("path is not allowed".to_string()));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // 同じ内容のものが既にあれば、書き直さない（開いているアプリがあっても失敗しない）
    if fs::read(&dest).is_ok_and(|existing| existing == content) {
        set_readonly(&dest, true)?;
        return Ok(dest);
    }
    if dest.exists() {
        // 前回書いた読み取り専用のファイルを上書きできるようにする
        set_readonly(&dest, false)?;
    }
    fs::write(&dest, &content)?;
    set_readonly(&dest, true)?;
    Ok(dest)
}

/// 読み取り専用属性を設定・解除する。
// Windows では属性の解除がこの方法しかない。対象はこのアプリが一時フォルダに書いたファイルだけ
#[allow(clippy::permissions_set_readonly_false)]
fn set_readonly(path: &Path, readonly: bool) -> std::io::Result<()> {
    let mut perm = fs::metadata(path)?.permissions();
    if perm.readonly() == readonly {
        return Ok(());
    }
    perm.set_readonly(readonly);
    fs::set_permissions(path, perm)
}

/// 配下の通常ファイルの読み取り専用を解除する（削除できるようにする）。シンボリックリンクは辿らない。
fn clear_readonly_recursive(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            clear_readonly_recursive(&entry.path());
        } else if meta.is_file() {
            let _ = set_readonly(&entry.path(), false);
        }
    }
}

/// 配下の通常ファイルの更新時刻のうち最も新しいもの。ファイルが無ければフォルダ自体の更新時刻。
fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    fn walk(dir: &Path, newest: &mut Option<SystemTime>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                walk(&entry.path(), newest);
            } else if meta.is_file() {
                if let Ok(t) = meta.modified() {
                    if newest.is_none_or(|n| t > n) {
                        *newest = Some(t);
                    }
                }
            }
        }
    }
    let mut newest = None;
    walk(dir, &mut newest);
    newest.or_else(|| fs::metadata(dir).and_then(|m| m.modified()).ok())
}

/// `root`（`<temp>/hikae-preview`）配下の、`max_age` を超えて更新されていない一時ファイルを削除する。
/// 構成は `<root>/<プロジェクト ID>/<短縮コミット>/...`。短縮コミット単位で削除し、空になった
/// プロジェクトのフォルダも片付ける。削除に失敗したもの（他のアプリで開いているなど）は残す。
/// 削除したフォルダ数を返す。`root` が無ければ 0。
pub fn cleanup_old_previews(root: &Path, max_age: Duration, now: SystemTime) -> usize {
    let Ok(projects) = fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0;
    for project in projects.flatten() {
        let project_dir = project.path();
        // リンクやファイルは辿らない・触らない
        if !fs::symlink_metadata(&project_dir).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let Ok(versions) = fs::read_dir(&project_dir) else {
            continue;
        };
        for version in versions.flatten() {
            let version_dir = version.path();
            if !fs::symlink_metadata(&version_dir).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let expired = newest_mtime(&version_dir)
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > max_age);
            if !expired {
                continue;
            }
            clear_readonly_recursive(&version_dir);
            if fs::remove_dir_all(&version_dir).is_ok() {
                removed += 1;
            }
        }
        // 空になったプロジェクトのフォルダだけを消す（中身があれば失敗して残る）
        let _ = fs::remove_dir(&project_dir);
    }
    removed
}
