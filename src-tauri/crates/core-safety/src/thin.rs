// 復元点（隠し ref）の間引き（設計書 6.2）。
//
// - `refs/hikae/snapshots/<ブランチ>/<時刻>`: 7 日間は全件、30 日までは 1 時間に 1 件、
//   それ以降は保持期間まで 1 日に 1 件。保持期間を過ぎたものは削除する
// - `refs/hikae/backup/<操作名>/<時刻>`: 保持期間（既定 90 日）を過ぎたものを削除する
// - 各グループ（ブランチ／操作名）で最も新しい復元点と、呼び出し側が指定した保護対象
//   （手動の操作に紐づく復元点）は、期間にかかわらず残す
// - 削除してよいのは上の 2 つの名前空間の ref のみ。それ以外の ref には一切触れない
//
// 何を消すかの判定（`plan_thinning`）は時刻を引数で受け取る純関数。実際の削除は
// `thin_restore_points` が `GitRunner` 経由の `update-ref -d` だけで行う（データの実体の削除は
// git の通常の gc に任せる）。

use crate::{SafetyError, BACKUP_REF_PREFIX, SNAPSHOT_REF_PREFIX};
use core_git::GitRunner;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

/// 自動保存の保持期間の既定（日。設計書 7章）
pub const DEFAULT_SNAPSHOT_RETENTION_DAYS: u32 = 90;
/// 操作前の控え（backup）の保持期間（日。設計書 6.2）
pub const BACKUP_RETENTION_DAYS: u32 = 90;

/// この日数までは自動保存を全件残す
const KEEP_ALL_DAYS: i64 = 7;
/// この日数までは 1 時間に 1 件に間引く（それ以降は保持期間まで 1 日に 1 件）
const HOURLY_UNTIL_DAYS: i64 = 30;

const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_HOUR: i64 = 3_600;

/// 間引きの方針
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinPolicy {
    /// 自動保存の保持期間（日）。設定（30 / 90 / 365）から渡す
    pub snapshot_retention_days: u32,
    /// 操作前の控えの保持期間（日）
    pub backup_retention_days: u32,
}

impl Default for ThinPolicy {
    fn default() -> Self {
        ThinPolicy {
            snapshot_retention_days: DEFAULT_SNAPSHOT_RETENTION_DAYS,
            backup_retention_days: BACKUP_RETENTION_DAYS,
        }
    }
}

/// 間引きの結果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThinReport {
    /// 削除した ref
    pub deleted: Vec<String>,
    /// 削除に失敗した ref（次回の間引きでやり直す）
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Snapshot,
    Backup,
}

struct Parsed<'a> {
    name: &'a str,
    kind: Kind,
    /// スナップショットはブランチ名、backup は操作名
    group: &'a str,
    at: OffsetDateTime,
}

/// ref 名の末尾の時刻（`YYYYMMDDTHHMMSSZ`、衝突時は `-<番号>` が付く）を読む。読めなければ None。
fn parse_ref_timestamp(s: &str) -> Option<OffsetDateTime> {
    let b = s.as_bytes();
    if b.len() < 16 || b[8] != b'T' || b[15] != b'Z' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i32> {
        let part = s.get(range)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let rest = &s[16..];
    if !(rest.is_empty()
        || (rest.starts_with('-')
            && rest.len() > 1
            && rest[1..].bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    let year = digits(0..4)?;
    let month = Month::try_from(u8::try_from(digits(4..6)?).ok()?).ok()?;
    let day = u8::try_from(digits(6..8)?).ok()?;
    let hour = u8::try_from(digits(9..11)?).ok()?;
    let minute = u8::try_from(digits(11..13)?).ok()?;
    let second = u8::try_from(digits(13..15)?).ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    Some(PrimitiveDateTime::new(date, time).assume_utc())
}

/// 復元点として解釈できる ref 名だけを返す。名前空間の外、グループ名が空、
/// 空白・制御文字を含む、時刻が読めない名前は None（= 間引きの対象にしない）。
fn parse_ref(name: &str) -> Option<Parsed<'_>> {
    let (kind, rest) = match name.strip_prefix(SNAPSHOT_REF_PREFIX) {
        Some(rest) => (Kind::Snapshot, rest),
        None => (Kind::Backup, name.strip_prefix(BACKUP_REF_PREFIX)?),
    };
    if name.chars().any(|c| c.is_control() || c.is_whitespace()) || name.contains("..") {
        return None;
    }
    let (group, stamp) = rest.rsplit_once('/')?;
    if group.is_empty() {
        return None;
    }
    let at = parse_ref_timestamp(stamp)?;
    Some(Parsed {
        name,
        kind,
        group,
        at,
    })
}

/// 間引きで削除する ref を決める（純関数。時刻は `now` で受け取る）。
///
/// `refs` には `refs/hikae/snapshots/` と `refs/hikae/backup/` の ref 名を渡す。それ以外の名前や
/// 解釈できない名前は削除対象にならない。`protected` の ref とグループごとの最新の 1 件は必ず残す。
/// 結果は名前順。
pub fn plan_thinning(
    refs: &[String],
    now: OffsetDateTime,
    policy: &ThinPolicy,
    protected: &HashSet<String>,
) -> Vec<String> {
    let now_ts = now.unix_timestamp();
    let snapshot_end = i64::from(policy.snapshot_retention_days) * SECS_PER_DAY;
    let hourly_end = (HOURLY_UNTIL_DAYS * SECS_PER_DAY).min(snapshot_end);
    let backup_end = i64::from(policy.backup_retention_days) * SECS_PER_DAY;

    // (種別, グループ) ごとに新しい順へ並べる
    let mut groups: BTreeMap<(Kind, &str), Vec<Parsed<'_>>> = BTreeMap::new();
    for parsed in refs.iter().filter_map(|r| parse_ref(r)) {
        groups
            .entry((parsed.kind, parsed.group))
            .or_default()
            .push(parsed);
    }

    let mut doomed = Vec::new();
    for ((kind, _), mut members) in groups {
        members.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.name.cmp(a.name)));

        // 期間にかかわらず残すもの: グループの最新の 1 件、保護対象、未来の時刻（時計のずれ）
        let forced: Vec<bool> = members
            .iter()
            .enumerate()
            .map(|(i, m)| i == 0 || protected.contains(m.name) || m.at.unix_timestamp() > now_ts)
            .collect();

        match kind {
            Kind::Backup => {
                for (m, forced) in members.iter().zip(&forced) {
                    if !forced && now_ts - m.at.unix_timestamp() > backup_end {
                        doomed.push(m.name.to_string());
                    }
                }
            }
            Kind::Snapshot => {
                // 区間ごとの枠（1 時間／1 日）。残すと決めたものが枠を使う
                let bucket_of = |age: i64, ts: i64| -> Option<(bool, i64)> {
                    if age < KEEP_ALL_DAYS * SECS_PER_DAY {
                        None
                    } else if age <= hourly_end {
                        Some((true, ts.div_euclid(SECS_PER_HOUR)))
                    } else {
                        Some((false, ts.div_euclid(SECS_PER_DAY)))
                    }
                };
                let mut used: HashSet<(bool, i64)> = HashSet::new();
                // 先に、必ず残すものが枠を使う
                for (m, forced) in members.iter().zip(&forced) {
                    if *forced {
                        let ts = m.at.unix_timestamp();
                        if let Some(b) = bucket_of(now_ts - ts, ts) {
                            used.insert(b);
                        }
                    }
                }
                // 新しい順に見て、枠が空いていれば残し、埋まっていれば削除する
                for (m, forced) in members.iter().zip(&forced) {
                    if *forced {
                        continue;
                    }
                    let ts = m.at.unix_timestamp();
                    let age = now_ts - ts;
                    if age > snapshot_end {
                        doomed.push(m.name.to_string());
                        continue;
                    }
                    // 7 日以内（枠なし）は残す。枠がすでに使われていれば削除する
                    if bucket_of(age, ts).is_some_and(|b| !used.insert(b)) {
                        doomed.push(m.name.to_string());
                    }
                }
            }
        }
    }
    doomed.sort();
    doomed
}

/// 復元点の名前空間の ref を列挙し、方針に従って古いものを削除する。
///
/// 削除は `update-ref -d` のみで、`refs/hikae/snapshots/` と `refs/hikae/backup/` の外の ref は
/// 呼び出し側の誤りがあっても削除しない（`GitRunner` の許可リストと二重に守る）。
pub fn thin_restore_points(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
    policy: &ThinPolicy,
    protected: &HashSet<String>,
) -> Result<ThinReport, SafetyError> {
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            SNAPSHOT_REF_PREFIX,
            BACKUP_REF_PREFIX,
        ],
    )?;
    let refs: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    let mut report = ThinReport::default();
    for name in plan_thinning(&refs, now, policy, protected) {
        if !name.starts_with(SNAPSHOT_REF_PREFIX) && !name.starts_with(BACKUP_REF_PREFIX) {
            report.failed.push(name);
            continue;
        }
        match runner.run_ok(repo, &["update-ref", "-d", &name]) {
            Ok(_) => report.deleted.push(name),
            Err(_) => report.failed.push(name),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn backup(op: &str, age: Duration) -> String {
        format!("{BACKUP_REF_PREFIX}{op}/{}", fmt(NOW - age))
    }

    fn plan(refs: &[String]) -> Vec<String> {
        plan_thinning(refs, NOW, &ThinPolicy::default(), &HashSet::new())
    }

    #[test]
    fn parses_timestamps_and_collision_suffix() {
        assert_eq!(
            parse_ref_timestamp("20261007T143045Z"),
            Some(datetime!(2026-10-07 14:30:45 UTC))
        );
        assert_eq!(
            parse_ref_timestamp("20261007T143045Z-2"),
            Some(datetime!(2026-10-07 14:30:45 UTC))
        );
        for bad in [
            "",
            "20261007T143045",
            "20261307T143045Z",
            "20261007T253045Z",
            "20261007T143045Zx",
            "20261007T143045Z-",
            "2026100aT143045Z",
        ] {
            assert_eq!(parse_ref_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn keeps_everything_within_seven_days() {
        let refs: Vec<String> = (0..20)
            .map(|i| snap("main", Duration::minutes(i * 10)))
            .chain((0..5).map(|d| snap("main", Duration::days(d) + Duration::hours(1))))
            .collect();
        assert!(plan(&refs).is_empty());
    }

    #[test]
    fn keeps_one_per_hour_between_seven_and_thirty_days() {
        // 10 日前の同じ時刻帯に 3 件、別の時刻帯に 1 件
        let base = Duration::days(10);
        let a = snap("main", base);
        let b = snap("main", base - Duration::minutes(10));
        let c = snap("main", base - Duration::minutes(20));
        let d = snap("main", base + Duration::hours(3));
        // 最新（グループの先頭）は別にある
        let newest = snap("main", Duration::minutes(5));
        let doomed = plan(&[a.clone(), b.clone(), c.clone(), d.clone(), newest]);
        // 同じ時刻帯では最も新しい c を残す
        let mut expect = vec![a, b];
        expect.sort();
        assert_eq!(doomed, expect);
    }

    #[test]
    fn keeps_one_per_day_after_thirty_days_until_retention() {
        let a = snap("main", Duration::days(45) + Duration::hours(2));
        let b = snap("main", Duration::days(45) + Duration::hours(1));
        let c = snap("main", Duration::days(60));
        let old = snap("main", Duration::days(91));
        let newest = snap("main", Duration::minutes(1));
        let doomed = plan(&[a.clone(), b, c, old.clone(), newest]);
        // a と b は同じ UTC 日（8/24）なので新しいほう（b）を残す。保持期間（90 日）を過ぎた old は削除
        let mut expect = vec![a, old];
        expect.sort();
        assert_eq!(doomed, expect);
    }

    #[test]
    fn retention_setting_extends_or_shortens_the_tail() {
        let at_200 = snap("main", Duration::days(200));
        let at_20 = snap("main", Duration::days(20));
        let newest = snap("main", Duration::minutes(1));
        let refs = [at_200.clone(), at_20.clone(), newest];

        let long = ThinPolicy {
            snapshot_retention_days: 365,
            ..ThinPolicy::default()
        };
        assert!(plan_thinning(&refs, NOW, &long, &HashSet::new()).is_empty());

        // 保持期間が 30 日なら、30 日を超えるものは削除（20 日前のものは 1 時間に 1 件として残る）
        let short = ThinPolicy {
            snapshot_retention_days: 30,
            ..ThinPolicy::default()
        };
        assert_eq!(
            plan_thinning(&refs, NOW, &short, &HashSet::new()),
            vec![at_200]
        );
    }

    #[test]
    fn newest_of_each_group_and_protected_refs_survive_any_age() {
        let main_old = snap("main", Duration::days(400));
        let feature_old = snap("feature/x", Duration::days(500));
        let feature_older = snap("feature/x", Duration::days(600));
        let protected_old = snap("main", Duration::days(450));
        let backup_old = backup("pull", Duration::days(300));
        let backup_older = backup("pull", Duration::days(310));
        let refs = [
            main_old,
            feature_old,
            feature_older.clone(),
            protected_old.clone(),
            backup_old,
            backup_older.clone(),
        ];
        let protected: HashSet<String> = [protected_old, backup_older.clone()].into();
        let doomed = plan_thinning(&refs, NOW, &ThinPolicy::default(), &protected);
        assert_eq!(doomed, vec![feature_older]);
    }

    #[test]
    fn backups_expire_after_retention_only() {
        let fresh = backup("restore", Duration::days(89));
        let stale = backup("restore", Duration::days(91));
        let newest = backup("restore", Duration::hours(1));
        let other_op_old = backup("pre-merge", Duration::days(200));
        let doomed = plan(&[fresh, stale.clone(), newest, other_op_old]);
        // pre-merge は唯一（= 最新）なので期間を過ぎていても残る
        assert_eq!(doomed, vec![stale]);
    }

    #[test]
    fn ignores_refs_it_does_not_own_or_cannot_read() {
        let newest = snap("main", Duration::minutes(1));
        let foreign = [
            "refs/heads/main".to_string(),
            "refs/tags/v1".to_string(),
            "refs/remotes/origin/main".to_string(),
            "refs/hikae/other/20200101T000000Z".to_string(),
            format!("{SNAPSHOT_REF_PREFIX}main/not-a-time"),
            format!("{SNAPSHOT_REF_PREFIX}20200101T000000Z"),
            format!("{BACKUP_REF_PREFIX}/20200101T000000Z"),
            format!("{BACKUP_REF_PREFIX}op/2020 0101T000000Z"),
        ];
        let mut refs = vec![newest];
        refs.extend(foreign);
        assert!(plan(&refs).is_empty());
    }

    #[test]
    fn future_timestamps_are_kept() {
        let future = format!(
            "{SNAPSHOT_REF_PREFIX}main/{}",
            fmt(NOW + Duration::days(400))
        );
        let newest = snap("main", Duration::minutes(1));
        let old = snap("main", Duration::days(100));
        assert_eq!(plan(&[future, newest, old.clone()]), vec![old]);
    }

    #[test]
    fn planning_is_idempotent() {
        let mut refs = Vec::new();
        for h in 0..24 * 100 {
            refs.push(snap("main", Duration::hours(h) + Duration::minutes(7)));
        }
        let first = plan(&refs);
        assert!(!first.is_empty());
        let kept: Vec<String> = refs.into_iter().filter(|r| !first.contains(r)).collect();
        assert!(plan(&kept).is_empty());
        // 残る件数の目安: 7 日 × 24 + 23 日 × 24 + 60 日 ≒ 780 件前後
        assert!(kept.len() < 900, "kept {}", kept.len());
    }

    #[test]
    fn groups_are_independent() {
        // ブランチ名に / を含んでも、別ブランチの最新が別ブランチの古い ref を守らない
        let a_new = snap("a", Duration::minutes(1));
        let a_old = snap("a", Duration::days(200));
        let b_only_old = snap("a/b", Duration::days(200));
        let doomed = plan(&[a_new, a_old.clone(), b_only_old]);
        assert_eq!(doomed, vec![a_old]);
    }
}
