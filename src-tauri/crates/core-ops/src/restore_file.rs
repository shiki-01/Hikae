// 1 ファイルの復元、元に戻すの取り消し、保存時点ごとの変更一覧（設計書 4.2、6.2、6.4）。
//
// - 状態を変更する操作は、必ず先に core-safety で復元点を作る（不変条件 3）
// - 戻す対象は `restore --source` のみ（`checkout -f` / `reset --hard` は使わない）
// - 指定時点に無いファイルは、いまのファイルを消さずに「戻せない」と返す（不変条件 6）

use crate::models::*;
use crate::open_path::validate_relative;
use crate::operations::{conflicts, current_branch, read_status};
use crate::pc_name::Meta;
use core_git::GitRunner;
use core_safety::{list_snapshots, BACKUP_REF_PREFIX, SNAPSHOT_REF_PREFIX};
use std::path::Path;
use time::OffsetDateTime;

/// フロントから渡されたプロジェクト内のパスを検査して、`/` 区切りの正規形にする。
/// 空・絶対パス・`..`・`.git` 配下・pathspec の特殊指定（先頭 `:`）を拒否する。
pub(crate) fn normalize_project_path(raw: &str) -> Result<String, OpsError> {
    validate_relative(raw)
        .map_err(|e| OpsError::InvalidInput(format!("path '{raw}' is not allowed: {e}")))?;
    let unified = raw.replace('\\', "/");
    let parts: Vec<&str> = unified
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.is_empty() {
        return Err(OpsError::InvalidInput("path is empty".to_string()));
    }
    if parts.iter().any(|p| p.eq_ignore_ascii_case(".git")) {
        return Err(OpsError::InvalidInput(
            "path must not point into .git".to_string(),
        ));
    }
    if parts[0].starts_with(':') {
        return Err(OpsError::InvalidInput(
            "path must not start with ':'".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

/// コミット指定を検査し、完全な OID に解決する。オプションに見える値は受け付けない。
pub(crate) fn resolve_commit(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
) -> Result<String, OpsError> {
    if rev.is_empty()
        || rev.starts_with('-')
        || rev.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(OpsError::InvalidInput("invalid commit".to_string()));
    }
    let spec = format!("{rev}^{{commit}}");
    let out = runner.run(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.code != 0 || oid.is_empty() {
        return Err(OpsError::InvalidInput("commit not found".to_string()));
    }
    Ok(oid)
}

/// 指定時点でのファイルサイズ。通常のファイルとして存在しなければ None。
pub(crate) fn size_at(
    runner: &GitRunner,
    repo: &Path,
    oid: &str,
    rel: &str,
) -> Result<Option<u64>, OpsError> {
    let out = runner.run_ok(repo, &["ls-tree", "-z", "-l", oid, "--", rel])?;
    Ok(crate::operations::parse_ls_tree_long(&out.stdout)
        .into_iter()
        .find(|f| f.path == rel)
        .map(|f| f.size))
}

/// pathspec のグロブ展開を避けるための指定
fn literal_pathspec(rel: &str) -> String {
    format!(":(literal){rel}")
}

/// 1 ファイルを戻した場合の影響を調べる（読み取りのみ）。
pub(crate) fn restore_file_preview(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
    path: &str,
) -> Result<RestoreFilePreview, OpsError> {
    let rel = normalize_project_path(path)?;
    let oid = resolve_commit(runner, repo, commit)?;
    let size_then = size_at(runner, repo, &oid, &rel)?;
    let now_meta = std::fs::metadata(repo.join(&rel))
        .ok()
        .filter(|m| m.is_file());
    let size_now = now_meta.as_ref().map(|m| m.len());

    let kind = if size_then.is_none() {
        RestoreFileKind::NotInThatPoint
    } else if now_meta.is_none() {
        RestoreFileKind::Recreate
    } else if is_identical_tracked(runner, repo, &oid, &rel)? {
        RestoreFileKind::Unchanged
    } else {
        RestoreFileKind::Overwrite
    };
    Ok(RestoreFilePreview {
        kind,
        size_now,
        size_then,
    })
}

/// 追跡中のファイルで、作業フォルダの内容が指定時点と同一か。
fn is_identical_tracked(
    runner: &GitRunner,
    repo: &Path,
    oid: &str,
    rel: &str,
) -> Result<bool, OpsError> {
    let spec = literal_pathspec(rel);
    let tracked = runner.run_ok(repo, &["ls-files", "-z", "--cached", "--", &spec])?;
    if tracked.stdout.is_empty() {
        return Ok(false);
    }
    let diff = runner.run(repo, &["diff", "--quiet", "--exit-code", oid, "--", &spec])?;
    Ok(diff.code == 0)
}

/// 保存対象外（.gitignore）で未追跡のファイルか。
fn is_ignored_untracked(runner: &GitRunner, repo: &Path, rel: &str) -> Result<bool, OpsError> {
    let out = runner.run(repo, &["check-ignore", "-q", "--", rel])?;
    Ok(out.code == 0)
}

/// 指定時点の 1 ファイルだけを作業フォルダに戻す。
///
/// 1. 指定時点に無ければ何も変更せず `NotInThatPoint`
/// 2. 上書きで失われる「保存対象外」ファイルがあれば何も変更せず `IgnoredFileInTheWay`
/// 3. 復元点を作る（未保存の変更を含む）
/// 4. `restore --source=<時点> --staged --worktree -- <path>`（他のファイルには触れない）
pub(crate) fn restore_file(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
    path: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<RestoreFileOutcome, OpsError> {
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    let rel = normalize_project_path(path)?;
    let oid = resolve_commit(runner, repo, commit)?;
    if size_at(runner, repo, &oid, &rel)?.is_none() {
        return Ok(RestoreFileOutcome::NotInThatPoint);
    }
    if repo.join(&rel).is_file() && is_ignored_untracked(runner, repo, &rel)? {
        return Ok(RestoreFileOutcome::IgnoredFileInTheWay);
    }

    let dirty_before = !read_status(runner, repo)?.entries.is_empty();
    let branch = current_branch(runner, repo)?;
    let point = meta.restore_point(runner, repo, &branch, "restore-file", now)?;

    let spec = literal_pathspec(&rel);
    runner.run_ok(
        repo,
        &[
            "restore",
            "--source",
            &oid,
            "--staged",
            "--worktree",
            "--",
            &spec,
        ],
    )?;

    let undo_ref = undo_target(runner, repo, &branch, point, dirty_before)?;
    Ok(RestoreFileOutcome::Restored { undo_ref })
}

/// 元に戻した後に、取り消しで使う復元点（`refs/hikae/` 配下の ref 名）を決める。
/// 取り消し先は「戻す直前の作業状態」。スナップショットが重複回避で作られなかった場合は、
/// 直近のスナップショットが同じ内容なのでそれを指す。変更が無かったなら HEAD の控えを指す。
pub(crate) fn undo_target(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    point: core_safety::RestorePoint,
    dirty_before: bool,
) -> Result<Option<String>, OpsError> {
    Ok(match point.snapshot {
        Some(s) => Some(s.ref_name),
        None if dirty_before => list_snapshots(runner, repo, branch)?
            .into_iter()
            .next()
            .map(|s| s.ref_name)
            .or(point.backup_ref),
        None => point.backup_ref,
    })
}

/// 復元点の指定（ref 名または完全な OID）を、復元点として登録されている OID に解決する。
/// `refs/hikae/snapshots/` と `refs/hikae/backup/` の ref に一致するものだけを受け付ける（不変条件 5）。
fn resolve_restore_point(runner: &GitRunner, repo: &Path, token: &str) -> Result<String, OpsError> {
    let is_ref = token.starts_with(SNAPSHOT_REF_PREFIX) || token.starts_with(BACKUP_REF_PREFIX);
    let is_oid =
        (token.len() == 40 || token.len() == 64) && token.chars().all(|c| c.is_ascii_hexdigit());
    if token.contains("..") || !(is_ref || is_oid) {
        return Err(OpsError::InvalidInput(
            "restore point must be a refs/hikae ref or a full object id".to_string(),
        ));
    }
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)",
            BACKUP_REF_PREFIX,
            SNAPSHOT_REF_PREFIX,
        ],
    )?;
    let token_lower = token.to_ascii_lowercase();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((name, oid)) = line.split_once('\t') {
            if name == token || (is_oid && oid == token_lower) {
                return Ok(oid.to_string());
            }
        }
    }
    Err(OpsError::InvalidInput(
        "restore point not found".to_string(),
    ))
}

/// 復元点の内容を作業フォルダに戻す（元に戻すの取り消し）。
/// 戻す前に、いまの状態の復元点を新たに作る。`reset --hard` / `checkout -f` は使わない。
/// 未追跡ファイル（復元点に無いもの）は消さない。
pub(crate) fn undo_restore(
    runner: &GitRunner,
    repo: &Path,
    restore_point: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<(), OpsError> {
    let oid = resolve_restore_point(runner, repo, restore_point)?;

    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    let branch = current_branch(runner, repo)?;
    let _ = meta.restore_point(runner, repo, &branch, "undo-restore", now)?;

    runner.run_ok(
        repo,
        &[
            "restore",
            "--source",
            &oid,
            "--staged",
            "--worktree",
            "--",
            ":/",
        ],
    )?;
    Ok(())
}

/// その保存で変更されたファイルの一覧（直前の保存との差、名前変更を検出）。
/// 最初の保存は全ファイルを「追加」として返す。結合（merge）は第 1 親との差。
pub(crate) fn list_point_changes(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
) -> Result<Vec<PointChange>, OpsError> {
    let oid = resolve_commit(runner, repo, commit)?;
    let parent_spec = format!("{oid}^");
    let parent = runner.run(repo, &["rev-parse", "--verify", "--quiet", &parent_spec])?;
    if parent.code != 0 {
        // 最初の保存。比べる親が無いので全ファイルを追加として扱う
        let out = runner.run_ok(repo, &["ls-tree", "-r", "-z", "--name-only", &oid])?;
        return Ok(out
            .stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| PointChange {
                path: String::from_utf8_lossy(p).into_owned(),
                old_path: None,
                kind: PointChangeKind::Added,
            })
            .collect());
    }
    let out = runner.run_ok(
        repo,
        &[
            "diff",
            "--name-status",
            "-M",
            "-z",
            &parent_spec,
            &oid,
            "--",
        ],
    )?;
    Ok(parse_name_status(&out.stdout))
}

/// `diff --name-status -M -z` の出力を解析する。
/// `<状態>\0<パス>\0`、名前変更・コピーは `<状態><類似度>\0<元>\0<先>\0`。
fn parse_name_status(stdout: &[u8]) -> Vec<PointChange> {
    let mut tokens = stdout
        .split(|b| *b == 0)
        .map(|t| String::from_utf8_lossy(t).into_owned());
    let mut changes = Vec::new();
    while let Some(status) = tokens.next() {
        if status.is_empty() {
            continue;
        }
        match status.chars().next() {
            Some('R') | Some('C') => {
                let (Some(old), Some(new)) = (tokens.next(), tokens.next()) else {
                    break;
                };
                if status.starts_with('R') {
                    changes.push(PointChange {
                        path: new,
                        old_path: Some(old),
                        kind: PointChangeKind::Renamed,
                    });
                } else {
                    // コピーは新しいファイルが増えたものとして扱う
                    changes.push(PointChange {
                        path: new,
                        old_path: None,
                        kind: PointChangeKind::Added,
                    });
                }
            }
            Some(c) => {
                let Some(path) = tokens.next() else { break };
                let kind = match c {
                    'A' => PointChangeKind::Added,
                    'D' => PointChangeKind::Deleted,
                    // M（内容）・T（種類）などは「更新」にまとめる
                    _ => PointChangeKind::Modified,
                };
                changes.push(PointChange {
                    path,
                    old_path: None,
                    kind,
                });
            }
            None => {}
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_plain_paths_and_unifies_separators() {
        assert_eq!(
            normalize_project_path("./docs//a b.txt").ok().as_deref(),
            Some("docs/a b.txt")
        );
        assert_eq!(
            normalize_project_path("資料\\報告.txt").ok().as_deref(),
            Some("資料/報告.txt")
        );
    }

    #[test]
    fn normalize_rejects_escapes_and_special_paths() {
        for bad in [
            "",
            ".",
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:\\x",
            ".git/config",
            "sub/.GIT/hooks/x",
            ":(top)x",
            "a\0b",
        ] {
            assert!(normalize_project_path(bad).is_err(), "{bad:?} must fail");
        }
    }

    #[test]
    fn name_status_parser_handles_rename_add_delete() {
        let raw = b"M\0a.txt\0A\0b c.txt\0D\0d.txt\0R087\0old.txt\0new.txt\0C100\0x\0y\0T\0t\0";
        let got = parse_name_status(raw);
        assert_eq!(got.len(), 6);
        assert_eq!(got[0].kind, PointChangeKind::Modified);
        assert_eq!(got[1].path, "b c.txt");
        assert_eq!(got[2].kind, PointChangeKind::Deleted);
        assert_eq!(got[3].kind, PointChangeKind::Renamed);
        assert_eq!(got[3].old_path.as_deref(), Some("old.txt"));
        assert_eq!(got[3].path, "new.txt");
        assert_eq!(got[4].kind, PointChangeKind::Added);
        assert_eq!(got[4].path, "y");
        assert_eq!(got[5].kind, PointChangeKind::Modified);
    }
}
