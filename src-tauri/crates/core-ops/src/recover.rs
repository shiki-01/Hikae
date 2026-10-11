// 中断された操作の復旧（設計書 5章 E15、6.3）。
//
// 保存・取り込み系の操作が途中で止まったとき、その操作が始まる直前に作られた復元点へ
// 作業フォルダを戻す。
//
// - `reset --hard` / `checkout -f` は使わない。戻し方は「元に戻すの取り消し」（`undo_restore`）と同じで、
//   `restore --source=<復元点> --staged --worktree` のみ。戻す前に、いまの状態の復元点を新たに作る
// - 途中で止まった取り込み（`MERGE_HEAD` が残っている）は、先に `merge --abort` で取り込みを取り消す
//   （これも復元点を作ってから行う）
// - 復元点は journal に残っていない（操作が完了しなかったため）。そのため、操作の名前と開始時刻から
//   `refs/hikae/backup/<操作名>/<時刻>` を探す。見つからなければ、何も起きていないと確かめられたときだけ
//   `NothingChanged`、そうでなければ何も変更せず `RestorePointNotFound`（設計書 6.3）
// - 何も起きていないかの判定（`check_interrupted`）は読み取りだけで行う。起動時の自動確定にも使う
// - 未追跡ファイルは消さない

use crate::models::{OpsError, RecoverDiagnosis, RecoverOutcome, SkipWorktreeReport};
use crate::operations::{abort_merge, conflicts, current_branch};
use crate::pc_name::Meta;
use crate::restore_file::undo_restore;
use crate::skip_worktree::reapply_skip_worktree;
use core_git::GitRunner;
use core_safety::{BACKUP_REF_PREFIX, SNAPSHOT_REF_PREFIX};
use std::path::Path;
use time::OffsetDateTime;

/// 復旧できる操作（ジャーナルの操作名）と、その操作が復元点に使う操作名
const RECOVERABLE: &[(&str, &str)] = &[
    ("save", "save"),
    ("pull", "pull"),
    // アップロードは、拒否されたとき内部で取り込みを行う（復元点は "pull"）
    ("push", "pull"),
    ("resolve", "resolve"),
];

/// ジャーナルの操作名が、復旧の対象（保存・取り込み系）か。
pub fn is_recoverable_operation(operation: &str) -> bool {
    RECOVERABLE.iter().any(|(op, _)| *op == operation)
}

/// ref 名末尾の時刻（`YYYYMMDDTHHMMSSZ`、衝突時は `-N` が付く）を Unix 秒にする。
fn parse_ref_timestamp(last_segment: &str) -> Option<i64> {
    let base = last_segment.split('-').next()?;
    let b = base.as_bytes();
    if b.len() != 16 || b[8] != b'T' || b[15] != b'Z' {
        return None;
    }
    if !b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| base[r].parse::<i32>().ok();
    let date = time::Date::from_calendar_date(
        num(0..4)?,
        time::Month::try_from(u8::try_from(num(4..6)?).ok()?).ok()?,
        u8::try_from(num(6..8)?).ok()?,
    )
    .ok()?;
    let t = time::Time::from_hms(
        u8::try_from(num(9..11)?).ok()?,
        u8::try_from(num(11..13)?).ok()?,
        u8::try_from(num(13..15)?).ok()?,
    )
    .ok()?;
    Some(
        time::PrimitiveDateTime::new(date, t)
            .assume_utc()
            .unix_timestamp(),
    )
}

/// 復旧に使う復元点を選ぶ。
///
/// 1. `operation` が始まった時刻（`started_at_unix`）以降に作られた、その操作の backup ref のうち最新のもの
/// 2. その復元点と同じ時刻の自動保存（未保存の変更を含む）があればそれ、無ければ
///    同じ HEAD から作られた直近の自動保存（内容が直前の自動保存と同じだったため新規に作られなかった場合）、
///    それも無ければ backup ref（そのときの HEAD）
fn select_restore_point(
    runner: &GitRunner,
    repo: &Path,
    operation: &str,
    started_at_unix: i64,
) -> Result<Option<String>, OpsError> {
    let Some(&(_, backup_op)) = RECOVERABLE.iter().find(|(op, _)| *op == operation) else {
        return Err(OpsError::InvalidInput(format!(
            "operation '{operation}' cannot be recovered"
        )));
    };

    // 開始以降に作られた backup ref のうち最新のもの
    let backup_prefix = format!("{BACKUP_REF_PREFIX}{backup_op}/");
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)",
            &backup_prefix,
        ],
    )?;
    let mut best: Option<(i64, String, String)> = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((name, oid)) = line.split_once('\t') else {
            continue;
        };
        let Some(ts) = name
            .strip_prefix(&backup_prefix)
            .and_then(parse_ref_timestamp)
        else {
            continue;
        };
        if ts < started_at_unix {
            continue;
        }
        let newer = best
            .as_ref()
            .is_none_or(|(t, n, _)| (ts, name) > (*t, n.as_str()));
        if newer {
            best = Some((ts, name.to_string(), oid.to_string()));
        }
    }
    let Some((backup_ts, backup_ref, backup_oid)) = best else {
        return Ok(None);
    };

    // 同じ時刻の自動保存、なければ同じ HEAD から作られた直近の自動保存
    let branch = current_branch(runner, repo)?;
    let snapshot_prefix = format!("{SNAPSHOT_REF_PREFIX}{branch}/");
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(parent)",
            &snapshot_prefix,
        ],
    )?;
    let mut same_time: Option<String> = None;
    let mut same_head: Option<(i64, String)> = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let (name, parent) = line.split_once('\t').unwrap_or((line, ""));
        let Some(ts) = name
            .strip_prefix(&snapshot_prefix)
            .and_then(parse_ref_timestamp)
        else {
            continue;
        };
        if ts == backup_ts {
            same_time = Some(name.to_string());
            break;
        }
        if ts < backup_ts && parent == backup_oid && same_head.as_ref().is_none_or(|(t, _)| ts > *t)
        {
            same_head = Some((ts, name.to_string()));
        }
    }
    Ok(Some(
        same_time
            .or_else(|| same_head.map(|(_, name)| name))
            .unwrap_or(backup_ref),
    ))
}

/// 取り込みの途中（`MERGE_HEAD` が残っている）か。
fn merge_in_progress(runner: &GitRunner, repo: &Path) -> Result<bool, OpsError> {
    let out = runner.run(repo, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])?;
    Ok(out.code == 0)
}

/// 作業フォルダの HEAD（完全な OID）。コミットが無ければ None。読み取りのみ。
pub(crate) fn current_head(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    let out = runner.run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if out.code != 0 {
        return Ok(None);
    }
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!oid.is_empty()).then_some(oid))
}

/// 中断された操作の記録のうち、復旧と判定に使う項目（ジャーナルから取り出したもの）
#[derive(Debug, Clone, Copy)]
pub(crate) struct InterruptedOp<'a> {
    /// ジャーナルの操作名（`pull` など）
    pub operation: &'a str,
    /// 操作の開始時刻（Unix 秒）
    pub started_at_unix: i64,
    /// 操作の開始時点の HEAD。記録が無ければ None
    pub head_before: Option<&'a str>,
}

/// 中断された操作が「何も変えていない」と言えるかの判定の内訳（読み取りのみ）。
pub struct InterruptedCheck {
    /// 技術詳細に出す内訳
    pub diagnosis: RecoverDiagnosis,
    /// 未解決の変更のぶつかりが残っているか
    pub unresolved_conflicts: bool,
}

impl InterruptedCheck {
    /// 作業フォルダ・インデックス・履歴に何も起きていないと確かめられたか（設計書 6.3 の自動確定の条件）。
    /// 次の (a)〜(c) がすべて真のとき。
    /// - (a) 開始時刻以降に作られた backup ref と snapshot ref が 1 件もない
    /// - (b) 開始時の HEAD の記録があり、いまの HEAD と一致する
    /// - (c) `MERGE_HEAD` が無く、未解決の変更のぶつかりも無い
    pub fn no_change_proven(&self) -> bool {
        let d = &self.diagnosis;
        d.backup_refs_after_start == 0
            && d.snapshot_refs_after_start == 0
            && d.head_changed() == Some(false)
            && !d.merge_head
            && !self.unresolved_conflicts
    }
}

/// 開始時刻以降に作られた復元点（backup ref と snapshot ref）の件数を数える。
/// 時刻を読めない ref は、安全側に「作られた」として数える。
fn restore_points_since(
    runner: &GitRunner,
    repo: &Path,
    started_at_unix: i64,
) -> Result<(usize, usize), OpsError> {
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            BACKUP_REF_PREFIX,
            SNAPSHOT_REF_PREFIX,
        ],
    )?;
    let (mut backups, mut snapshots) = (0, 0);
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let name = line.trim();
        let is_recent = name
            .rsplit('/')
            .next()
            .and_then(parse_ref_timestamp)
            .is_none_or(|ts| ts >= started_at_unix);
        if !is_recent {
            continue;
        }
        if name.starts_with(BACKUP_REF_PREFIX) {
            backups += 1;
        } else if name.starts_with(SNAPSHOT_REF_PREFIX) {
            snapshots += 1;
        }
    }
    Ok((backups, snapshots))
}

/// 中断された操作が、作業フォルダ・インデックス・履歴を変えていないかを調べる（読み取りのみ）。
/// 復旧の前にも、起動時の自動確定の前にも使う。何も変更しない。
pub(crate) fn check_interrupted(
    runner: &GitRunner,
    repo: &Path,
    started_at_unix: i64,
    head_before: Option<&str>,
) -> Result<InterruptedCheck, OpsError> {
    let (backups, snapshots) = restore_points_since(runner, repo, started_at_unix)?;
    let diagnosis = RecoverDiagnosis {
        backup_refs_after_start: backups,
        snapshot_refs_after_start: snapshots,
        head_before: head_before.map(str::to_string),
        head_now: current_head(runner, repo)?,
        merge_head: merge_in_progress(runner, repo)?,
    };
    let unresolved_conflicts = !conflicts(runner, repo)?.is_empty();
    Ok(InterruptedCheck {
        diagnosis,
        unresolved_conflicts,
    })
}

/// 中断された操作の直前の復元点へ作業フォルダを戻す。
///
/// 復元点が見つからないときは、`check_interrupted` で何も起きていないと確かめられれば
/// `NothingChanged`（作業フォルダには触れない）、確かめられなければ `RestorePointNotFound`（内訳つき）を返す。
/// どちらの場合も、戻す・書き換える処理は行わない。
pub(crate) fn recover_interrupted(
    runner: &GitRunner,
    repo: &Path,
    interrupted: InterruptedOp<'_>,
    now: OffsetDateTime,
    meta: Meta,
    skip: &mut SkipWorktreeReport,
) -> Result<RecoverOutcome, OpsError> {
    let InterruptedOp {
        operation,
        started_at_unix,
        head_before,
    } = interrupted;
    let Some(restore_point) = select_restore_point(runner, repo, operation, started_at_unix)?
    else {
        let check = check_interrupted(runner, repo, started_at_unix, head_before)?;
        if check.no_change_proven() && is_recoverable_operation(operation) {
            return Ok(RecoverOutcome::NothingChanged);
        }
        return Err(OpsError::RestorePointNotFound(check.diagnosis));
    };

    // 途中で止まった取り込みを先に取り消す（復元点を作ってから merge --abort）
    if merge_in_progress(runner, repo)? {
        abort_merge(runner, repo, now, meta)?;
    }

    // 戻す前にいまの状態の復元点を作り、復元点の内容へ戻す
    undo_restore(runner, repo, &restore_point, now, meta)?;
    // 無視中のファイルの退避を書き戻し、印を付け直す（設計書 4.8）。取り消しの後に行う
    skip.absorb(reapply_skip_worktree(runner, repo)?);
    Ok(RecoverOutcome::Restored { restore_point })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_timestamps_are_parsed_with_or_without_a_collision_suffix() {
        let t = parse_ref_timestamp("20261007T143045Z");
        let expected = time::Date::from_calendar_date(2026, time::Month::October, 7)
            .expect("date")
            .with_hms(14, 30, 45)
            .expect("time")
            .assume_utc()
            .unix_timestamp();
        assert_eq!(t, Some(expected));
        assert_eq!(parse_ref_timestamp("20261007T143045Z-2"), t);
        assert_eq!(parse_ref_timestamp("main"), None);
        assert_eq!(parse_ref_timestamp("20261307T143045Z"), None);
    }
}
