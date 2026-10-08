// 新規ファイルの「元に戻す（作成しない）」（設計書 4.2、6.4。安全上の不変条件 6 の唯一の例外）。
//
// 利用者が明示的に選んだときに限り、まだ一度も保存されていない新規ファイル 1 件を、作業フォルダから
// OS のごみ箱へ移す。完全には削除しない。次の安全策をすべて満たしたときだけ実行する。
//
// 1. 対象は「単一の未追跡ファイル」だけ。フォルダ・リンク・追跡済み・保存対象外・プロジェクト外は拒否
// 2. 実行前に復元点（自動保存。一時インデックスで作るため作業フォルダとインデックスは変わらない）を作り、
//    復元点にそのファイルの内容が入っていることを、内容の比較で確かめる。確かめられなければ削除しない
// 3. 復元点に含められない大きさ（自動保存から除外される大きさ）のファイルは、復元できないため削除しない
// 4. 削除の直前に、未追跡であることをもう一度確かめる
// 5. 削除は `Trasher`（ごみ箱への移動）だけで行う。ごみ箱が使えないときは削除せずエラーにする
//
// 戻り値の `undo_ref` は復元点の ref 名。`undo_restore` でこの操作の前の状態に戻せる。

use crate::models::{DiscardRefusal, OpsError};
use crate::open_path::{resolve_entry_in_project, OpenPathError};
use crate::operations::{conflicts, current_branch};
use crate::pc_name::Meta;
use crate::restore_file::{normalize_project_path, undo_target};
use crate::size_check::SizeLimits;
use core_git::GitRunner;
use core_safety::list_snapshots;
use std::path::Path;
use time::OffsetDateTime;

/// 復元点の操作名（`refs/hikae/backup/<操作名>/`）
const OPERATION: &str = "discard-new-file";

/// ファイルをごみ箱へ移す処理。OS ごとの実装は呼び出し側（`app`）が持ち、テストでは別の実装に差し替える。
pub trait Trasher: Send + Sync {
    /// `path` のファイル 1 件をごみ箱へ移す。完全には削除しない。移せなければ `Err`（ファイルは残る）
    fn trash(&self, path: &Path) -> std::io::Result<()>;
}

/// 「作成しない」にした結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedNewFile {
    /// 取り消し用の復元点（`refs/hikae/` 配下の ref 名）。`undo_restore` にそのまま渡せる
    pub undo_ref: Option<String>,
}

pub(crate) fn discard_new_file(
    runner: &GitRunner,
    repo: &Path,
    path: &str,
    limits: SizeLimits,
    trasher: &dyn Trasher,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<DiscardedNewFile, OpsError> {
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    // 字句検査（`..`・絶対パス・`.git` 配下を拒否）と、親フォルダの実体検査（リンクで外へ出ない）。
    // 対象そのものはリンクかもしれないため解決しない
    let rel = normalize_project_path(path)?;
    let file = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let refuse = |reason: DiscardRefusal| OpsError::DiscardRefused {
        reason,
        file: file.clone(),
    };

    let abs = match resolve_entry_in_project(repo, &rel) {
        Ok(abs) => abs,
        Err(OpenPathError::NotFound) => return Err(refuse(DiscardRefusal::NotAFile)),
        Err(OpenPathError::Io(e) | OpenPathError::RootUnavailable(e)) => return Err(e.into()),
        Err(other) => {
            return Err(OpsError::InvalidInput(format!(
                "path '{rel}' is not allowed: {other}"
            )))
        }
    };

    // 通常のファイルだけ（リンクはたどらない。`symlink_metadata` はリンクそのものを見る）
    let size = match std::fs::symlink_metadata(&abs) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => return Err(refuse(DiscardRefusal::NotAFile)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(refuse(DiscardRefusal::NotAFile))
        }
        Err(e) => return Err(e.into()),
    };

    // 自動保存から除外される大きさは、復元点に入らず復元できない。削除しない
    if size > limits.warn_bytes {
        return Err(refuse(DiscardRefusal::TooLarge { size }));
    }

    if !is_untracked_file(runner, repo, &rel)? {
        return Err(refuse(DiscardRefusal::NotUntracked));
    }

    // 復元点を作る（不変条件 3）。一時インデックスで作るため、作業フォルダとインデックスは変わらない（不変条件 4）
    let branch = current_branch(runner, repo)?;
    let point = meta.restore_point(runner, repo, &branch, OPERATION, now)?;

    // 復元点にファイルの内容が入っていることを確かめる。同じ内容の自動保存が既にあって新しい
    // 復元点が作られなかったときは、直近の自動保存を調べる
    let snapshot_commit = match &point.snapshot {
        Some(s) => s.commit.clone(),
        None => match list_snapshots(runner, repo, &branch)?.into_iter().next() {
            Some(s) => s.commit,
            None => return Err(refuse(DiscardRefusal::NotBackedUp)),
        },
    };
    let stored = runner.run(
        repo,
        &["cat-file", "-p", &format!("{snapshot_commit}:{rel}")],
    )?;
    if stored.code != 0 || stored.stdout != std::fs::read(&abs)? {
        return Err(refuse(DiscardRefusal::NotBackedUp));
    }

    // 削除の直前に、通常のファイルのままで、まだ未追跡であることを再確認する
    let still_regular = std::fs::symlink_metadata(&abs)
        .map(|m| m.is_file())
        .unwrap_or(false);
    if !still_regular || !is_untracked_file(runner, repo, &rel)? {
        return Err(refuse(DiscardRefusal::NotUntracked));
    }

    // ごみ箱へ移す。完全には削除しない（不変条件 6 の例外）
    if let Err(e) = trasher.trash(&abs) {
        return Err(if crate::file_in_use::is_sharing_violation(&e) {
            OpsError::FileInUse {
                file: Some(file.clone()),
            }
        } else {
            refuse(DiscardRefusal::TrashFailed)
        });
    }
    // 移したはずのファイルが残っているなら、成功とは扱わない
    if std::fs::symlink_metadata(&abs).is_ok() {
        return Err(refuse(DiscardRefusal::TrashFailed));
    }

    let undo_ref = undo_target(runner, repo, &branch, point, true)?;
    Ok(DiscardedNewFile { undo_ref })
}

/// `rel` が、保存もインデックスへの登録もされておらず、保存対象外でもないファイルとして
/// `git ls-files -o --exclude-standard` に出るか。
fn is_untracked_file(runner: &GitRunner, repo: &Path, rel: &str) -> Result<bool, OpsError> {
    let spec = format!(":(literal){rel}");
    let out = runner.run_ok(
        repo,
        &["ls-files", "-z", "-o", "--exclude-standard", "--", &spec],
    )?;
    Ok(out.stdout.split(|b| *b == 0).any(|p| p == rel.as_bytes()))
}
