// 自動保存のクラウド控え（設計書 6.4）。
//
// ローカルの `refs/hikae/snapshots/` を、保存先（origin）へ `git push origin <ref>:<同じ ref>` で送る。
// - 送るのは自動保存の名前空間だけ（`refs/hikae/backup/` は送らない）
// - 固定の形の refspec を 100 件ずつ 1 回の push にまとめる。形の検査は core-git の許可リストが行う
// - `--force`・削除（`:<ref>`・`--delete`）は使わない。クラウド側の ref は消さない（不変条件 1）
// - 送り済みの ref は git が何もしない。新しく作られた ref は `--porcelain` の `*` 行で数える
//   （人間向けの出力は読まない。不変条件・既定値「機械可読形式」）
// - 失敗は push と同じ経路（`GitError` → `OpsError`）で返す。分類と再試行は app 層が行う
// - 設定・ログイン・保存先の有無は app 層が判断する。この関数は呼ばれたら送る

use crate::models::OpsError;
use core_git::{GitError, GitRunner};
use core_safety::SNAPSHOT_REF_PREFIX;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 1 回の push にまとめる ref の数
pub const CLOUD_BACKUP_BATCH: usize = 100;

/// クラウド控えの結果
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CloudBackupOutcome {
    /// この呼び出しで、保存先に新しく作った自動保存の ref の数（送り済みのものは数えない）
    pub pushed_refs: u32,
    /// ローカルにある自動保存の ref の総数（送る対象）
    pub total_refs: u32,
}

/// ローカルの自動保存の ref を、保存先へ控える。
pub(crate) fn backup_snapshots(
    runner: &GitRunner,
    repo: &Path,
) -> Result<CloudBackupOutcome, OpsError> {
    let refs = list_snapshot_refs(runner, repo)?;
    let total_refs = u32::try_from(refs.len()).unwrap_or(u32::MAX);
    let mut pushed_refs: u32 = 0;

    for batch in refs.chunks(CLOUD_BACKUP_BATCH) {
        // 送り元と送り先は同じ ref（`<ref>:<ref>`）。`+`・削除・別名の形は作らない
        let specs: Vec<String> = batch.iter().map(|r| format!("{r}:{r}")).collect();
        let mut args: Vec<&str> = vec!["push", "--porcelain", "origin"];
        args.extend(specs.iter().map(String::as_str));

        let out = runner.run(repo, &args)?;
        if out.code != 0 {
            // push と同じ経路で分類する（オフライン・認証などは app 層が判定する）
            return Err(OpsError::from(GitError::Failed {
                code: out.code,
                stderr: out.stderr,
                args_summary: format!("push origin <{} snapshot refs>", batch.len()),
            }));
        }
        pushed_refs = pushed_refs.saturating_add(count_new_refs(&out.stdout));
    }

    Ok(CloudBackupOutcome {
        pushed_refs,
        total_refs,
    })
}

/// ローカルにある自動保存の ref の数（読み取りのみ。画面の表示に使う）。
pub(crate) fn local_snapshot_count(runner: &GitRunner, repo: &Path) -> Result<u32, OpsError> {
    let n = list_snapshot_refs(runner, repo)?.len();
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `refs/hikae/snapshots/` の下の ref 名を列挙する（読み取りのみ）。
/// 送れる形（許可リストの固定の形）に合わない名前は送らず、一覧から外す。
fn list_snapshot_refs(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, OpsError> {
    let out = runner.run_ok(
        repo,
        &["for-each-ref", "--format=%(refname)", SNAPSHOT_REF_PREFIX],
    )?;
    let refs = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter(|name| core_git::is_snapshot_cloud_refspec(&format!("{name}:{name}")))
        .map(str::to_string)
        .collect();
    Ok(refs)
}

/// `--porcelain` の出力のうち、新しく作られた ref の行（先頭の印が `*`）の数。
/// 行の形は `<印>\t<送り元>:<送り先>\t<要約>`。送り済み（`=`）や更新（` `）は数えない。
fn count_new_refs(stdout: &[u8]) -> u32 {
    let n = String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| line.starts_with("*\t"))
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_new_reference_lines() {
        let out = b"To /tmp/x\n*\trefs/hikae/snapshots/main/a:refs/hikae/snapshots/main/a\t[new reference]\n=\trefs/hikae/snapshots/main/b:refs/hikae/snapshots/main/b\t[up to date]\n \trefs/hikae/snapshots/main/c:refs/hikae/snapshots/main/c\tabc..def\nDone\n";
        assert_eq!(count_new_refs(out), 1);
        assert_eq!(count_new_refs(b""), 0);
    }
}
