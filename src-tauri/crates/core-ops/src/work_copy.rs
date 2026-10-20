// 作業コピー（別の案を試す場所。設計書 4.9）。内部では Git のブランチを使う。
//
// - 一覧は読み取りのみ。`wc-` で始まるものが作業コピー、それ以外が元のもの
// - 作る・切り替える・採用するは状態を変える。どれも、未保存の変更が無いことを確かめてから
//   復元点を先に作り（不変条件 3）、そのあとに branch / checkout / merge を行う
// - 切り替えは `checkout <ブランチ>`（`--detach` を渡さない）。`reset --hard`・`checkout -f`・
//   `branch -d/-D`・`stash` は使わない（不変条件 1）
// - 同一プロジェクトの直列実行（不変条件 7）は app 層の直列キューで保つ

use crate::models::{OpsError, PullOutcome, SkipWorktreeReport, WorkCopyInfo};
use crate::operations::{
    ahead_behind_range, conflicts, current_branch, merge_incoming, read_status,
};
use crate::pc_name::Meta;
use core_git::GitRunner;
use std::collections::HashSet;
use std::path::Path;
use time::{OffsetDateTime, UtcOffset};

/// 作業コピーの内部名の接頭辞。これで始まるブランチが作業コピー、それ以外は元のもの
const PREFIX: &str = "wc-";

/// 同じ秒に作られた内部名の衝突を `-1`、`-2` … で避けるときの上限。超えたら作らない
const MAX_SUFFIX: u32 = 1000;

fn is_work_copy(branch: &str) -> bool {
    branch.starts_with(PREFIX)
}

/// ローカルのブランチ名（`refs/heads/` の下）を読む（読み取りのみ）
fn local_branches(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, OpsError> {
    let out = runner.run_ok(
        repo,
        &["for-each-ref", "--format=%(refname)", "refs/heads/"],
    )?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().strip_prefix("refs/heads/"))
        .map(str::to_string)
        .collect())
}

/// 現在のブランチ名。detached HEAD のときは None（読み取りのみ）
fn current_branch_opt(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    let out = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    if out.code != 0 {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// HEAD の完全な OID。まだ保存が無い（HEAD が無い）ときは None
fn head_oid(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    let out = runner.run(repo, &["rev-parse", "--verify", "-q", "HEAD"])?;
    if out.code != 0 {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// 未保存の変更（未追跡を含む）があるか
fn has_unsaved(runner: &GitRunner, repo: &Path) -> Result<bool, OpsError> {
    Ok(!read_status(runner, repo)?.entries.is_empty())
}

/// 未保存の変更があれば、何も変えずに断る（設計書 4.9）
fn refuse_if_unsaved(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    if has_unsaved(runner, repo)? {
        return Err(OpsError::UnsavedChanges);
    }
    Ok(())
}

/// 未解決のぶつかりが残っていれば、何も変えずに断る（既存の保存・取り込みと同じ）
fn refuse_if_conflicted(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    let current = conflicts(runner, repo)?;
    if !current.is_empty() {
        return Err(OpsError::Conflict(current));
    }
    Ok(())
}

/// 作業コピーの一覧（読み取りのみ）。並びは Git の既定（ブランチ名の順）
pub(crate) fn list(runner: &GitRunner, repo: &Path) -> Result<Vec<WorkCopyInfo>, OpsError> {
    let current = current_branch_opt(runner, repo)?;
    let infos = local_branches(runner, repo)?
        .into_iter()
        .map(|branch| WorkCopyInfo {
            is_current: current.as_deref() == Some(branch.as_str()),
            is_original: !is_work_copy(&branch),
            branch,
        })
        .collect();
    Ok(infos)
}

/// 内部名の時刻部分（UTC の `YYYYMMDD-HHMMSS`）
fn stamp(now: OffsetDateTime) -> String {
    let utc = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        utc.year(),
        utc.month() as u8,
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// 既存の名前と衝突しない内部名を決める（`wc-YYYYMMDD-HHMMSS`、衝突なら `-1`、`-2` …）
fn unused_name(existing: &HashSet<String>, now: OffsetDateTime) -> Result<String, OpsError> {
    let base = format!("{PREFIX}{}", stamp(now));
    if !existing.contains(&base) {
        return Ok(base);
    }
    for n in 1..=MAX_SUFFIX {
        let candidate = format!("{base}-{n}");
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(OpsError::Unexpected(
        "too many work copies were created in the same second".to_string(),
    ))
}

/// 作業コピーを作る（設計書 4.9「作る」）。
/// 未保存の変更があれば何も変えず `UnsavedChanges`。復元点（操作名 `work-copy-create`）を作ってから、
/// 現在の HEAD の OID で内部名のブランチを作り、切り替える。作った内部名を返す。
pub(crate) fn create(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<String, OpsError> {
    refuse_if_conflicted(runner, repo)?;
    refuse_if_unsaved(runner, repo)?;
    let Some(head) = head_oid(runner, repo)? else {
        // 保存が 1 件も無いプロジェクトには、作業コピーの元になる版が無い
        return Err(OpsError::InvalidInput(
            "no saved version to start a work copy from".to_string(),
        ));
    };
    let current = current_branch(runner, repo)?;
    let existing: HashSet<String> = local_branches(runner, repo)?.into_iter().collect();
    let name = unused_name(&existing, now)?;

    // 状態を変える前に復元点を作る（不変条件 3）
    meta.restore_point(runner, repo, &current, "work-copy-create", now)?;

    // 内部名のブランチを、いまの HEAD と同じ版で作る（branch の固定の形）
    runner.run_ok(repo, &["branch", &name, &head])?;
    // 切り替える。`--detach` は付けない（detached HEAD を作らない）
    runner.run_ok(repo, &["checkout", &name])?;
    Ok(name)
}

/// 作業コピーへ切り替える（設計書 4.9「切り替える」）。`branch` は `refs/heads` に実在するブランチであること。
/// 未保存の変更があれば何も変えず `UnsavedChanges`。復元点（操作名 `work-copy-switch`）の後に `checkout`。
/// `checkout` が拒否したとき（未追跡ファイルの上書きなど）は、ファイルを変えずにエラーを返す。
pub(crate) fn switch(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<(), OpsError> {
    refuse_if_conflicted(runner, repo)?;
    // 一覧に無い名前（`-` で始まる名前を含む）は、checkout に渡さず拒否する
    if !local_branches(runner, repo)?.iter().any(|b| b == branch) {
        return Err(OpsError::InvalidInput(
            "the work copy does not exist".to_string(),
        ));
    }
    switch_to(runner, repo, branch, now, meta, "work-copy-switch")
}

/// 切り替えの本体（検査済みのブランチ名を前提にする）。すでにそのブランチなら何もしない。
/// `operation` は復元点の操作名（切り替えは `work-copy-switch`、採用の前の切り替えも同じ）。
fn switch_to(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    now: OffsetDateTime,
    meta: Meta,
    operation: &str,
) -> Result<(), OpsError> {
    let current = current_branch_opt(runner, repo)?;
    if current.as_deref() == Some(branch) {
        return Ok(());
    }
    refuse_if_unsaved(runner, repo)?;
    let current = current.ok_or_else(|| {
        OpsError::Unexpected("the current branch could not be read (detached HEAD)".to_string())
    })?;

    meta.restore_point(runner, repo, &current, operation, now)?;
    // `--detach` は付けない。未追跡ファイルの上書きなどで git が拒否すれば、ファイルは変わらない
    runner.run_ok(repo, &["checkout", branch])?;
    Ok(())
}

/// 作業コピーを元の作業コピーに取り込む（設計書 4.9「採用する」）。
///
/// 手順: 元の作業コピーへ切り替える（切り替えと同じ規則。復元点 `work-copy-switch`）→ 復元点
/// （`work-copy-adopt`）→ `merge`。取り込む内容が無ければ何もせず `UpToDate`。ぶつかりは `Conflicted`
/// として返し、解消（4.4）の流れに乗せる。無視中のファイルの扱いは取り込み（4.3・4.8）と同じ。
pub(crate) fn adopt(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    original: &str,
    now: OffsetDateTime,
    meta: Meta,
    skip: &mut SkipWorktreeReport,
) -> Result<PullOutcome, OpsError> {
    refuse_if_conflicted(runner, repo)?;
    // 採用は「作業コピーを元へ取り込む」ため、名前の向きを検査する（逆向きや同じ名前は拒否）
    if !is_work_copy(branch) || is_work_copy(original) || branch == original {
        return Err(OpsError::InvalidInput(
            "adopt needs a work copy and the original work copy".to_string(),
        ));
    }
    let existing = local_branches(runner, repo)?;
    if !existing.iter().any(|b| b == branch) || !existing.iter().any(|b| b == original) {
        return Err(OpsError::InvalidInput(
            "the work copy does not exist".to_string(),
        ));
    }
    // 未保存の変更があれば、切り替えも取り込みもしない（元へ切り替える前に確かめる）
    refuse_if_unsaved(runner, repo)?;

    // 元の作業コピーへ切り替える（切り替えと同じ規則。すでに元なら何もしない）
    switch_to(runner, repo, original, now, meta, "work-copy-switch")?;

    // 取り込む内容が無ければ、復元点も作らず終える
    let (ahead, behind) = ahead_behind_range(runner, repo, &format!("HEAD...{branch}"))?;
    if behind == 0 {
        return Ok(PullOutcome::UpToDate);
    }

    // 状態を変える前に復元点を作る（不変条件 3）
    meta.restore_point(runner, repo, original, "work-copy-adopt", now)?;

    // 元だけに変更がなければ fast-forward。双方に変更があれば merge（ぶつかりは 4.4）
    let clean = merge_incoming(runner, repo, branch, ahead == 0, skip)?;
    if clean {
        if ahead == 0 {
            return Ok(PullOutcome::FastForwarded);
        }
        let commit = String::from_utf8_lossy(&runner.run_ok(repo, &["rev-parse", "HEAD"])?.stdout)
            .trim()
            .to_string();
        return Ok(PullOutcome::Merged { commit });
    }
    Ok(PullOutcome::Conflicted {
        files: conflicts(runner, repo)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month, Time};

    fn utc(y: i32, m: Month, d: u8, h: u8, mi: u8, s: u8) -> OffsetDateTime {
        OffsetDateTime::new_utc(
            Date::from_calendar_date(y, m, d).expect("date"),
            Time::from_hms(h, mi, s).expect("time"),
        )
    }

    #[test]
    fn stamp_is_utc_and_has_no_separators_other_than_the_one_hyphen() {
        // 東京時間の 12:34:56 は UTC の 03:34:56
        let dt = utc(2026, Month::October, 20, 3, 34, 56)
            .to_offset(UtcOffset::from_hms(9, 0, 0).expect("offset"));
        assert_eq!(stamp(dt), "20261020-033456");
    }

    #[test]
    fn unused_name_adds_numbers_on_collision() {
        let now = utc(2026, Month::October, 20, 3, 34, 56);
        let mut existing = HashSet::new();
        assert_eq!(unused_name(&existing, now).unwrap(), "wc-20261020-033456");
        existing.insert("wc-20261020-033456".to_string());
        assert_eq!(unused_name(&existing, now).unwrap(), "wc-20261020-033456-1");
        existing.insert("wc-20261020-033456-1".to_string());
        assert_eq!(unused_name(&existing, now).unwrap(), "wc-20261020-033456-2");
        // 生成した名前は内部名の形に一致する
        assert!(core_git::is_work_copy_name(
            &unused_name(&existing, now).unwrap()
        ));
    }
}
