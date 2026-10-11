// 自動保存（スナップショット）の容量上限（設計書 6.2）。
//
// - 上限 = max(snapshot_size_cap_x × P, SIZE_CAP_FLOOR_BYTES)。P は保存済みの木（HEAD^{tree}）の大きさ
// - S は、スナップショットの ref からだけ到達できるオブジェクトの大きさ（ブランチ・リモート追跡からは到達できないもの）。
//   集合の差で求める: S = usage(スナップショット ∪ ブランチ・リモート) − usage(ブランチ・リモート)（負なら 0）
// - S が上限を超えたら、保護されていないスナップショットを古い順に削除する。
//   1 ラウンドで残っている候補の古い方 25%（最低 1 件）を削除し、S を測り直す。最大 8 ラウンド。
//   ブランチ・リモート側の大きさは、削除で変わらないため 1 回だけ測って使い回す
// - 保護規則は期間による間引き（`plan_thinning`）と同じ（各ブランチの最新 1 件、利用者の手動の操作に紐づく
//   復元点、未来の時刻）。保護一覧を読めない場合の見送りは呼び出し側（app）で行う
// - 削除は `update-ref -d` だけ（`refs/hikae/snapshots/` の ref のみ）。データの実体の解放は git の通常の gc に任せる
// - 大きさの測定は `rev-list --objects --disk-usage`（読み取りのみ。作業フォルダ・インデックス・ブランチに触れない）
//
// 上限と削除対象の判定は、時刻・サイズを引数で受け取る純関数（`size_cap_limit`、`plan_size_round`）。

use crate::thin::{forced_flags, parse_ref, Kind, Parsed, ThinPolicy};
use crate::{head_exists, SafetyError, SNAPSHOT_REF_PREFIX};
use core_git::GitRunner;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use time::OffsetDateTime;

/// 自動保存の容量上限の既定（保存済みの木の大きさの倍数。設定の既定と一致。設計書 7.1）
pub const DEFAULT_SNAPSHOT_SIZE_CAP_X: u32 = 2;

/// 容量上限の下限（バイト）: 100 MiB。
/// 上限は「保存済みの木の大きさの倍数」なので、小さいプロジェクトでは上限も小さくなり、
/// 自動保存がほぼ全部消えてしまう。それを防ぐため、最低でもこの大きさまでは残す（確定仕様）。
pub const SIZE_CAP_FLOOR_BYTES: u64 = 100 * 1024 * 1024;

/// 容量の間引きを 1 回の実行で行う最大ラウンド数。各ラウンドの後に大きさを測り直す
pub const SIZE_CAP_MAX_ROUNDS: usize = 8;

/// 1 ラウンドで削除する割合（％）。残っている削除可能な候補の古い方から、最低 1 件
const SIZE_CAP_ROUND_PERCENT: usize = 25;

/// 容量上限（バイト）を求める（純関数）。`cap_x` が 0 のときは上限なし（None）。
/// 上限は `max(cap_x × project_bytes, floor_bytes)`。
pub fn size_cap_limit(project_bytes: u64, cap_x: u32, floor_bytes: u64) -> Option<u64> {
    if cap_x == 0 {
        return None;
    }
    Some(
        project_bytes
            .saturating_mul(u64::from(cap_x))
            .max(floor_bytes),
    )
}

/// 容量の間引きの 1 ラウンドで削除する snapshot ref を決める（純関数。時刻は `now` で受け取る）。
///
/// 対象は `refs/hikae/snapshots/` の ref だけ（backup は対象外）。各ブランチの最新 1 件、
/// `protected`、未来の時刻は残す。残りを古い順に並べ、先頭の 25%（切り捨て、最低 1 件）を返す。
/// 候補が無ければ空。結果は名前順。
pub fn plan_size_round(
    refs: &[String],
    now: OffsetDateTime,
    protected: &HashSet<String>,
) -> Vec<String> {
    let now_ts = now.unix_timestamp();
    let mut groups: BTreeMap<&str, Vec<Parsed<'_>>> = BTreeMap::new();
    for parsed in refs
        .iter()
        .filter_map(|r| parse_ref(r))
        .filter(|p| p.kind == Kind::Snapshot)
    {
        groups.entry(parsed.group).or_default().push(parsed);
    }

    let mut candidates: Vec<Parsed<'_>> = Vec::new();
    for mut members in groups.into_values() {
        members.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.name.cmp(a.name)));
        let forced = forced_flags(&members, now_ts, protected);
        candidates.extend(
            members
                .into_iter()
                .zip(forced)
                .filter(|(_, is_forced)| !*is_forced)
                .map(|(m, _)| m),
        );
    }

    // 古い順（同時刻は名前順）
    candidates.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.name.cmp(b.name)));
    let take = if candidates.is_empty() {
        0
    } else {
        (candidates.len() * SIZE_CAP_ROUND_PERCENT / 100).max(1)
    };
    let mut doomed: Vec<String> = candidates
        .iter()
        .take(take)
        .map(|m| m.name.to_string())
        .collect();
    doomed.sort();
    doomed
}

/// スナップショットとブランチ・リモート追跡の和集合から到達できるオブジェクトの大きさ（バイト）。
/// `rev-list --objects --disk-usage --glob=refs/hikae/snapshots/* --branches --remotes`（読み取りのみ）
pub fn snapshot_and_branch_bytes(runner: &GitRunner, repo: &Path) -> Result<u64, SafetyError> {
    let glob = format!("--glob={SNAPSHOT_REF_PREFIX}*");
    let out = runner.run_ok(
        repo,
        &[
            "rev-list",
            "--objects",
            "--disk-usage",
            glob.as_str(),
            "--branches",
            "--remotes",
        ],
    )?;
    parse_bytes(&out.stdout)
}

/// ブランチ・リモート追跡から到達できるオブジェクトの大きさ（バイト）。
/// `rev-list --objects --disk-usage --branches --remotes`（読み取りのみ）
pub fn branch_reachable_bytes(runner: &GitRunner, repo: &Path) -> Result<u64, SafetyError> {
    let out = runner.run_ok(
        repo,
        &[
            "rev-list",
            "--objects",
            "--disk-usage",
            "--branches",
            "--remotes",
        ],
    )?;
    parse_bytes(&out.stdout)
}

/// S（スナップショットだけが参照するオブジェクトの大きさ）を、2 つの測定値の差で求める（純関数）。
/// 差が負になるとき（測定の丸めなど）は 0。
pub fn only_snapshot_bytes(union_bytes: u64, branch_bytes: u64) -> u64 {
    union_bytes.saturating_sub(branch_bytes)
}

/// スナップショットだけが参照しているオブジェクトの大きさ（バイト。`S`）を、2 回の測定で求める。
/// `S = usage(スナップショット ∪ ブランチ・リモート) − usage(ブランチ・リモート)`。
///
/// ブランチ側は、スナップショットの作成後に同じ内容を保存しても正しく除外される（集合の差で測るため）。
/// 容量の間引き（`thin_snapshots_by_size`）は、ブランチ側の測定を 1 回だけ行って使い回す。
pub fn snapshot_only_bytes(runner: &GitRunner, repo: &Path) -> Result<u64, SafetyError> {
    let union = snapshot_and_branch_bytes(runner, repo)?;
    let branches = branch_reachable_bytes(runner, repo)?;
    Ok(only_snapshot_bytes(union, branches))
}

/// 保存済みの木（HEAD^{tree}）の大きさ（バイト。`P`）。HEAD が無ければ None（測れないので何もしない）。
pub fn project_tree_bytes(runner: &GitRunner, repo: &Path) -> Result<Option<u64>, SafetyError> {
    if !head_exists(runner, repo)? {
        return Ok(None);
    }
    let out = runner.run_ok(
        repo,
        &["rev-list", "--objects", "--disk-usage", "HEAD^{tree}"],
    )?;
    parse_bytes(&out.stdout).map(Some)
}

fn parse_bytes(stdout: &[u8]) -> Result<u64, SafetyError> {
    String::from_utf8_lossy(stdout)
        .trim()
        .parse::<u64>()
        .map_err(|_| SafetyError::Unexpected("failed to read the size of objects".to_string()))
}

fn list_snapshot_refs(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, SafetyError> {
    let out = runner.run_ok(
        repo,
        &["for-each-ref", "--format=%(refname)", SNAPSHOT_REF_PREFIX],
    )?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// 容量の間引きの結果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SizeCapReport {
    /// 上限（バイト）。上限なし（`snapshot_size_cap_x` が 0）または HEAD が無いときは None で、何もしない
    pub limit_bytes: Option<u64>,
    /// 間引きを始める前の S（バイト）
    pub before_bytes: u64,
    /// 最後に測った S（バイト）
    pub after_bytes: u64,
    /// 実行したラウンド数
    pub rounds: usize,
    /// 削除した ref（名前順ではなく削除した順）
    pub deleted: Vec<String>,
    /// 削除に失敗した ref（同じ実行の中では再度選ばない）
    pub failed: Vec<String>,
    /// 上限を超えたまま終わった（保護されたものしか残らない、または最大ラウンド数で打ち切った）
    pub over_limit: bool,
    /// 途中で一覧・測定に失敗して止めた。それまでの削除は `deleted` に残っている
    pub stopped_by_error: bool,
}

/// S が上限を超えたとき、保護されていないスナップショットを古い順に削除する。
///
/// 削除は `update-ref -d`（`refs/hikae/snapshots/` の ref のみ）。`protected` の ref と、
/// 各ブランチの最新 1 件、未来の時刻は残す。期間による間引き（`thin_restore_points`）を先に行い、
/// そのあとに呼ぶ想定。上限を超えていなければ何も削除しない。
pub fn thin_snapshots_by_size(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
    policy: &ThinPolicy,
    protected: &HashSet<String>,
) -> Result<SizeCapReport, SafetyError> {
    let mut report = SizeCapReport::default();
    if policy.snapshot_size_cap_x == 0 {
        return Ok(report);
    }
    let Some(project) = project_tree_bytes(runner, repo)? else {
        return Ok(report);
    };
    let Some(limit) = size_cap_limit(
        project,
        policy.snapshot_size_cap_x,
        policy.size_cap_floor_bytes,
    ) else {
        return Ok(report);
    };
    report.limit_bytes = Some(limit);

    // スナップショットが 1 件も無ければ S は 0 で、測る必要がない
    if list_snapshot_refs(runner, repo)?.is_empty() {
        return Ok(report);
    }
    // ブランチ・リモート側の大きさは 1 回だけ測り、ラウンドの測り直しで使い回す。
    // 削除するのはスナップショットの ref だけなので、この値はラウンドの間に変わらない
    let branches = branch_reachable_bytes(runner, repo)?;
    let before = only_snapshot_bytes(snapshot_and_branch_bytes(runner, repo)?, branches);
    report.before_bytes = before;
    report.after_bytes = before;
    if before <= limit {
        return Ok(report);
    }

    // ここから先は削除が起きうるため、エラーは返さず report に残す（削除した ref を呼び出し側が忘れないように）
    let mut current = before;
    let mut failed: HashSet<String> = HashSet::new();
    for _ in 0..SIZE_CAP_MAX_ROUNDS {
        let refs: Vec<String> = match list_snapshot_refs(runner, repo) {
            Ok(refs) => refs.into_iter().filter(|r| !failed.contains(r)).collect(),
            Err(_) => {
                report.stopped_by_error = true;
                break;
            }
        };
        let victims = plan_size_round(&refs, now, protected);
        if victims.is_empty() {
            break;
        }
        for name in victims {
            match runner.run_ok(repo, &["update-ref", "-d", &name]) {
                Ok(_) => report.deleted.push(name),
                Err(_) => {
                    failed.insert(name.clone());
                    report.failed.push(name);
                }
            }
        }
        report.rounds += 1;
        match snapshot_and_branch_bytes(runner, repo) {
            Ok(union) => {
                let bytes = only_snapshot_bytes(union, branches);
                current = bytes;
                report.after_bytes = bytes;
            }
            Err(_) => {
                report.stopped_by_error = true;
                break;
            }
        }
        if current <= limit {
            break;
        }
    }
    report.over_limit = !report.stopped_by_error && current > limit;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BACKUP_REF_PREFIX;
    use time::macros::datetime;
    use time::Duration;

    const NOW: OffsetDateTime = datetime!(2026-10-08 12:00:00 UTC);

    fn fmt(t: OffsetDateTime) -> String {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            t.year(),
            t.month() as u8,
            t.day(),
            t.hour(),
            t.minute(),
            t.second()
        )
    }

    fn snap(branch: &str, age: Duration) -> String {
        format!("{SNAPSHOT_REF_PREFIX}{branch}/{}", fmt(NOW - age))
    }

    fn plan(refs: &[String], protected: &HashSet<String>) -> Vec<String> {
        plan_size_round(refs, NOW, protected)
    }

    #[test]
    fn limit_is_multiple_of_project_size_with_floor() {
        // 上限なし
        assert_eq!(size_cap_limit(1_000, 0, 100), None);
        // 倍数が下限より大きい
        assert_eq!(size_cap_limit(1_000, 1, 100), Some(1_000));
        assert_eq!(size_cap_limit(1_000, 2, 100), Some(2_000));
        assert_eq!(size_cap_limit(1_000, 5, 100), Some(5_000));
        // 倍数が下限より小さいときは下限
        assert_eq!(size_cap_limit(10, 2, 100), Some(100));
        assert_eq!(size_cap_limit(0, 5, 100), Some(100));
        // 溢れない
        assert_eq!(size_cap_limit(u64::MAX, 5, 100), Some(u64::MAX));
    }

    #[test]
    fn default_floor_is_100_mib() {
        assert_eq!(SIZE_CAP_FLOOR_BYTES, 100 * 1024 * 1024);
        assert_eq!(
            size_cap_limit(0, 2, SIZE_CAP_FLOOR_BYTES),
            Some(SIZE_CAP_FLOOR_BYTES)
        );
    }

    #[test]
    fn empty_input_plans_nothing() {
        assert!(plan(&[], &HashSet::new()).is_empty());
    }

    #[test]
    fn removes_a_quarter_of_candidates_with_a_minimum_of_one() {
        // 候補の数 → 1 ラウンドの削除数（25% 切り捨て、最低 1）
        for (candidates, expected) in [(1, 1), (3, 1), (4, 1), (7, 1), (8, 2), (11, 2), (40, 10)] {
            // 最新 1 件（残る）＋ candidates 件の古い復元点
            let mut refs = vec![snap("main", Duration::minutes(1))];
            refs.extend((0..candidates).map(|i| snap("main", Duration::days(10 + i))));
            let doomed = plan(&refs, &HashSet::new());
            assert_eq!(doomed.len(), expected, "候補 {candidates} 件");
        }
    }

    #[test]
    fn removes_the_oldest_candidates_first() {
        let newest = snap("main", Duration::minutes(1));
        let ages: Vec<Duration> = (0..8).map(|i| Duration::days(10 + i)).collect();
        let refs: Vec<String> = std::iter::once(newest)
            .chain(ages.iter().map(|age| snap("main", *age)))
            .collect();
        // 8 件のうち古い 2 件（20 日前・19 日前）
        let mut expect = vec![
            snap("main", Duration::days(17)),
            snap("main", Duration::days(16)),
        ];
        expect.sort();
        assert_eq!(plan(&refs, &HashSet::new()), expect);
    }

    #[test]
    fn newest_per_branch_protected_and_future_are_never_candidates() {
        let main_new = snap("main", Duration::minutes(1));
        let main_protected = snap("main", Duration::days(50));
        let main_old = snap("main", Duration::days(60));
        let feature_only = snap("feature/x", Duration::days(70));
        // 未来の時刻は別のブランチに置く（同じブランチに置くと、時刻が最も新しいものとして main の最新扱いになるため）
        let future = format!("{SNAPSHOT_REF_PREFIX}work/{}", fmt(NOW + Duration::days(9)));
        let refs = [
            main_new,
            main_protected.clone(),
            main_old.clone(),
            feature_only.clone(),
            future.clone(),
        ];
        let protected: HashSet<String> = [main_protected.clone()].into();
        // 候補は main_old だけ（feature_only はブランチの唯一の ref、future は未来、main_protected は保護）
        assert_eq!(plan(&refs, &protected), vec![main_old]);
        assert!(!plan(&refs, &protected).contains(&feature_only));
        assert!(!plan(&refs, &protected).contains(&future));
        assert!(!plan(&refs, &protected).contains(&main_protected));
    }

    #[test]
    fn backups_and_foreign_refs_are_ignored() {
        let newest = snap("main", Duration::minutes(1));
        let backup_old = format!("{BACKUP_REF_PREFIX}pull/{}", fmt(NOW - Duration::days(300)));
        let heads = "refs/heads/feature/old".to_string();
        let broken = format!("{SNAPSHOT_REF_PREFIX}main/not-a-time");
        let refs = [newest, backup_old, heads, broken];
        assert!(plan(&refs, &HashSet::new()).is_empty());
    }

    #[test]
    fn only_snapshot_is_the_difference_of_the_two_measurements() {
        // 和集合が大きいぶんがスナップショットだけの大きさ
        assert_eq!(only_snapshot_bytes(1_500, 1_000), 500);
        // 同じなら 0（スナップショットだけの内容が無い）
        assert_eq!(only_snapshot_bytes(1_000, 1_000), 0);
        // 差が負（測定の丸めなど）は 0 とする
        assert_eq!(only_snapshot_bytes(900, 1_000), 0);
        assert_eq!(only_snapshot_bytes(0, u64::MAX), 0);
    }
}
