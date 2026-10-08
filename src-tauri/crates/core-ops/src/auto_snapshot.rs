// ファイル監視による自動保存（スナップショット。設計書 6.2、7章「自動保存」）。
//
// 一時インデックス（`GIT_INDEX_FILE`）で作業フォルダ全体のスナップショットを作り、作業フォルダ・
// インデックスは一切変更しない（`core-safety` の `create_snapshot_excluding`）。
// - 変更が無い、または直前のスナップショット・`HEAD` と内容が同じなら作らない（重複を作らない）
// - 大きいファイル（保存前のサイズ検査の対象になるもの）は、自動では履歴に入れない。追跡済みの
//   ファイルは `HEAD` の内容のまま、未追跡のファイルは含めない。利用者が保存するときに、
//   通常のサイズ検査と選択（4.1）を通る
// - ぶつかりの解消中は作らない（ぶつかりのマーカーが入った途中の状態を自動保存にしない）

use crate::models::OpsError;
use crate::operations::{conflicts, read_status};
use crate::pc_name::Meta;
use crate::size_check::{self, LargeFile, SizeLimits};
use core_git::GitRunner;
use serde::{Deserialize, Serialize};
use std::path::Path;
use time::OffsetDateTime;

/// 自動保存を作らなかった理由
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AutoSnapshotSkip {
    /// 変更のぶつかりが未解消
    Conflict,
    /// ブランチ上にいない（detached HEAD）
    DetachedHead,
}

/// 自動保存の結果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AutoSnapshotOutcome {
    /// 作った
    Created {
        /// 作った自動保存の ref 名（`refs/hikae/snapshots/...`）
        snapshot_ref: String,
        /// 大きいため自動保存に含めなかったファイル
        excluded_large: Vec<LargeFile>,
    },
    /// 作る必要が無かった（変更なし、または直前の自動保存・`HEAD` と同じ内容）
    Unchanged,
    /// 今は作れない
    Skipped(AutoSnapshotSkip),
}

pub(crate) fn auto_snapshot(
    runner: &GitRunner,
    repo: &Path,
    limits: SizeLimits,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<AutoSnapshotOutcome, OpsError> {
    if !conflicts(runner, repo)?.is_empty() {
        return Ok(AutoSnapshotOutcome::Skipped(AutoSnapshotSkip::Conflict));
    }

    let branch_out = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    if branch_out.code != 0 {
        return Ok(AutoSnapshotOutcome::Skipped(AutoSnapshotSkip::DetachedHead));
    }
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    // 変更が無ければ、一時インデックスを作る前に終える（全ファイルを読み直さずに済む）
    if read_status(runner, repo)?.entries.is_empty() {
        return Ok(AutoSnapshotOutcome::Unchanged);
    }

    // 保存前と同じ検査で見つかる大きいファイル（警告・保存不可の両方）は含めない
    let findings = size_check::scan(runner, repo, limits)?;
    let mut excluded_large: Vec<LargeFile> = findings
        .blocked
        .into_iter()
        .chain(findings.warned)
        .collect();
    excluded_large.sort_by(|a, b| a.path.cmp(&b.path));
    let exclude: Vec<String> = excluded_large.iter().map(|f| f.path.clone()).collect();

    let snapshot = core_safety::create_snapshot_excluding(
        runner,
        repo,
        &branch,
        now,
        meta.signature,
        &exclude,
    )
    .map_err(OpsError::from)?;

    Ok(match snapshot {
        Some(s) => AutoSnapshotOutcome::Created {
            snapshot_ref: s.ref_name,
            excluded_large,
        },
        None => AutoSnapshotOutcome::Unchanged,
    })
}
