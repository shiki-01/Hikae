//! 自動保存の容量上限の統合テスト（設計書 6.2）。実リポジトリを使い、時刻は引数で渡す（実時間を使わない）。
//! 圧縮されにくい擬似乱数の内容を書き、オブジェクトの大きさを確実に積み上げる。

mod common;

use core_git::GitRunner;
use core_safety::{
    create_snapshot, project_tree_bytes, snapshot_only_bytes, thin_snapshots_by_size, ThinPolicy,
    SIZE_CAP_MAX_ROUNDS, SNAPSHOT_REF_PREFIX,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

const BASE: OffsetDateTime = datetime!(2026-10-01 00:00:00 UTC);
const NOW: OffsetDateTime = datetime!(2026-10-08 12:00:00 UTC);
/// スナップショット 1 件が増やす大きさの目安（作業フォルダの work.bin）
const BLOB: usize = 100_000;
/// 保存済みの木（HEAD）に入れる大きさ。P の目安
const BIG: usize = 200_000;

/// 圧縮されにくい擬似乱数。seed が違えば別の内容になる
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let out = common::git_command(repo, args);
    assert!(out.status.success(), "git {args:?} 失敗");
    String::from_utf8(out.stdout).expect("UTF-8 でない出力")
}

/// HEAD が main の、大きな内容を 1 件コミット済みのリポジトリ
fn setup() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path().to_path_buf();
    common::git_command(&repo, &["init", "-b", "main"]);
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("user.email", "t@example.com"),
        ("user.name", "T"),
    ] {
        common::git_command(&repo, &["config", "--local", k, v]);
    }
    fs::write(repo.join("big.bin"), noise(0, BIG)).expect("write");
    common::git_command(&repo, &["add", "-A"]);
    common::git_command(&repo, &["commit", "-m", "init"]);
    (temp, repo)
}

/// work.bin を seed の内容に変えてから、指定時刻のスナップショットを作る。ref 名を返す
fn snapshot_with(runner: &GitRunner, repo: &Path, at: OffsetDateTime, seed: u64) -> String {
    fs::write(repo.join("work.bin"), noise(seed, BLOB)).expect("write");
    create_snapshot(runner, repo, "main", at)
        .expect("snapshot")
        .expect("snapshot should be created")
        .ref_name
}

/// 古い順（BASE から 1 時間ずつ）に n 件のスナップショットを作る
fn make_snapshots(runner: &GitRunner, repo: &Path, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| snapshot_with(runner, repo, BASE + Duration::hours(i as i64), i as u64 + 1))
        .collect()
}

fn snapshot_refs(repo: &Path) -> Vec<String> {
    git_stdout(
        repo,
        &["for-each-ref", "--format=%(refname)", SNAPSHOT_REF_PREFIX],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

fn policy(cap_x: u32) -> ThinPolicy {
    // 下限は小さくして、倍数（P × cap_x）で上限が決まるようにする
    ThinPolicy {
        snapshot_size_cap_x: cap_x,
        size_cap_floor_bytes: 1,
        ..ThinPolicy::default()
    }
}

/// 作業フォルダ・インデックス・ブランチ・HEAD の履歴の状態を比べるための記録
struct Fingerprint {
    worktree: BTreeMap<String, (String, u64)>,
    index: Vec<u8>,
    status: String,
    branches: String,
    log: String,
}

fn fingerprint(repo: &Path) -> Fingerprint {
    Fingerprint {
        worktree: common::hash_working_tree(repo).expect("worktree"),
        index: common::read_index_file(repo).expect("index"),
        status: git_stdout(repo, &["status", "--porcelain=v2"]),
        branches: git_stdout(repo, &["for-each-ref", "refs/heads/", "refs/tags/"]),
        log: git_stdout(repo, &["log", "--format=%H", "HEAD"]),
    }
}

#[test]
fn over_the_cap_removes_oldest_first_and_keeps_the_newest() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    // 12 件 × 約 100KB ＝ 約 1.2MB。上限は 5 × 約 200KB ＝ 約 1MB
    let created = make_snapshots(&runner, &repo, 12);
    let newest = created.last().expect("newest").clone();
    let before = fingerprint(&repo);

    let report =
        thin_snapshots_by_size(&runner, &repo, NOW, &policy(5), &HashSet::new()).expect("thin");

    let limit = report.limit_bytes.expect("上限が決まっている");
    assert!(report.before_bytes > limit, "前提: 上限を超えている");
    assert!(report.after_bytes <= limit, "上限以下まで整理される");
    assert!(!report.over_limit && !report.stopped_by_error);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(!report.deleted.is_empty());

    let remaining = snapshot_refs(&repo);
    assert_eq!(remaining.len(), created.len() - report.deleted.len());
    assert!(remaining.contains(&newest), "最新の 1 件は残る");
    for d in &report.deleted {
        assert!(!remaining.contains(d), "削除されたはず: {d}");
    }
    // 古い順: 削除したものは、残した他のどれよりも古い（created は古い順に並んでいる）
    let index_of = |r: &String| created.iter().position(|c| c == r).expect("created");
    let max_deleted = report.deleted.iter().map(index_of).max().expect("deleted");
    let min_kept_other = remaining
        .iter()
        .filter(|r| **r != newest)
        .map(index_of)
        .min()
        .expect("kept");
    assert!(max_deleted < min_kept_other, "古い順に削除されていない");

    // 報告した大きさは、測り直した値と一致する
    assert_eq!(
        snapshot_only_bytes(&runner, &repo).expect("measure"),
        report.after_bytes
    );

    // ブランチ・履歴・作業フォルダ・インデックスは変わらない
    assert_eq!(
        before.branches,
        git_stdout(&repo, &["for-each-ref", "refs/heads/", "refs/tags/"])
    );
    assert_eq!(
        before.log,
        git_stdout(&repo, &["log", "--format=%H", "HEAD"])
    );
    assert_eq!(
        before.status,
        git_stdout(&repo, &["status", "--porcelain=v2"])
    );
    assert_eq!(
        before.index,
        common::read_index_file(&repo).expect("index after")
    );
    assert_eq!(
        before.worktree,
        common::hash_working_tree(&repo).expect("worktree after")
    );
}

#[test]
fn protected_snapshots_survive_the_size_cap() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    let created = make_snapshots(&runner, &repo, 12);
    let newest = created.last().expect("newest").clone();
    // 手動の操作に紐づく復元点（保護一覧に入るもの）を、古い側に 2 件置く
    let protected: HashSet<String> = [created[0].clone(), created[1].clone()].into();

    let report = thin_snapshots_by_size(&runner, &repo, NOW, &policy(5), &protected).expect("thin");

    assert!(!report.deleted.is_empty());
    for d in &report.deleted {
        assert!(!protected.contains(d), "保護対象を削除した: {d}");
        assert_ne!(d, &newest, "最新を削除した");
    }
    let remaining = snapshot_refs(&repo);
    for kept in protected.iter().chain(std::iter::once(&newest)) {
        assert!(remaining.contains(kept), "残るはず: {kept}");
    }
    assert!(!report.over_limit, "保護対象以外の候補で上限以下にできる");
}

#[test]
fn within_the_cap_nothing_is_deleted() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    let created = make_snapshots(&runner, &repo, 3);
    let before = snapshot_refs(&repo);

    let report =
        thin_snapshots_by_size(&runner, &repo, NOW, &policy(5), &HashSet::new()).expect("thin");

    assert!(report.limit_bytes.is_some());
    assert!(report.before_bytes <= report.limit_bytes.expect("limit"));
    assert_eq!(report.rounds, 0);
    assert!(report.deleted.is_empty() && report.failed.is_empty());
    assert_eq!(snapshot_refs(&repo), before);
    assert_eq!(before.len(), created.len());
}

#[test]
fn cap_zero_means_no_cap_and_nothing_is_deleted() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    make_snapshots(&runner, &repo, 12);
    let before = snapshot_refs(&repo);

    let report =
        thin_snapshots_by_size(&runner, &repo, NOW, &policy(0), &HashSet::new()).expect("thin");

    assert!(report.limit_bytes.is_none());
    assert_eq!(report.rounds, 0);
    assert!(report.deleted.is_empty());
    assert_eq!(snapshot_refs(&repo), before);
}

#[test]
fn without_a_head_nothing_is_measured_or_deleted() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path().to_path_buf();
    common::git_command(&repo, &["init", "-b", "main"]);
    let runner = GitRunner::from_path_env();
    fs::write(repo.join("work.bin"), noise(1, BLOB)).expect("write");
    create_snapshot(&runner, &repo, "main", BASE)
        .expect("snapshot")
        .expect("created");
    let before = snapshot_refs(&repo);
    assert_eq!(before.len(), 1);

    assert_eq!(project_tree_bytes(&runner, &repo).expect("P"), None);
    let report =
        thin_snapshots_by_size(&runner, &repo, NOW, &policy(1), &HashSet::new()).expect("thin");

    assert!(report.limit_bytes.is_none());
    assert!(report.deleted.is_empty());
    assert_eq!(snapshot_refs(&repo), before);
}

#[test]
fn stops_after_the_maximum_number_of_rounds() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    // 40 件 × 約 100KB ＝ 約 4MB。上限は P × 1 ＝ 約 200KB。1 ラウンドで候補の 25% ずつ減るため、8 ラウンドでは収まらない
    make_snapshots(&runner, &repo, 40);

    let report =
        thin_snapshots_by_size(&runner, &repo, NOW, &policy(1), &HashSet::new()).expect("thin");

    assert_eq!(report.rounds, SIZE_CAP_MAX_ROUNDS);
    assert!(report.over_limit, "上限を超えたまま打ち切る");
    assert!(report.after_bytes > report.limit_bytes.expect("limit"));
    // 各ラウンドは残りの候補の 25%（切り捨て、最低 1 件）。
    // 候補 39 → 9 件、30 → 7、23 → 5、18 → 4、14 → 3、11 → 2、9 → 2、7 → 1 で合計 33 件
    assert_eq!(report.deleted.len(), 33);
    assert_eq!(snapshot_refs(&repo).len(), 40 - 33);
    assert!(!report.stopped_by_error);
}

#[test]
fn measures_only_objects_that_only_snapshots_reach() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    let p0 = project_tree_bytes(&runner, &repo)
        .expect("P")
        .expect("HEAD があるので測れる");
    assert!(p0 >= BIG as u64, "P は HEAD の木の大きさ: {p0}");
    assert_eq!(
        snapshot_only_bytes(&runner, &repo).expect("S"),
        0,
        "スナップショットが無いとき 0"
    );

    // スナップショットだけが持つ内容（ブランチには無い）
    snapshot_with(&runner, &repo, BASE, 7);
    let s1 = snapshot_only_bytes(&runner, &repo).expect("S");
    assert!(
        s1 >= BLOB as u64,
        "スナップショットだけの内容は S に入る: {s1}"
    );

    // ブランチの履歴（スナップショットの親に含まれる）にある内容は S に入らない。P には入る
    // work.bin（seed 7）は含めない（含めると、そのスナップショットの内容もブランチから到達できてしまう）
    fs::write(repo.join("keep.bin"), noise(8, BLOB)).expect("write");
    common::git_command(&repo, &["add", "--", "keep.bin"]);
    common::git_command(&repo, &["commit", "-m", "keep"]);
    let p1 = project_tree_bytes(&runner, &repo)
        .expect("P")
        .expect("HEAD");
    assert!(
        p1 >= p0 + BLOB as u64 - 1_000,
        "P は保存済みの木の増えた分を含む: {p1}"
    );
    // 作業フォルダの work.bin を変えたスナップショット。keep.bin はその親（HEAD）に含まれるので、
    // S には新しい work.bin の分（約 1 件分）だけが増え、keep.bin の分は増えない
    snapshot_with(&runner, &repo, BASE + Duration::hours(1), 9);
    let s2 = snapshot_only_bytes(&runner, &repo).expect("S");
    assert!(
        s2 >= s1 + BLOB as u64 - 1_000,
        "新しい work.bin の分が増える: {s2}"
    );
    assert!(
        s2 < s1 + BLOB as u64 + 10_000,
        "親に含まれる keep.bin は S に入らない: {s2}（前 {s1}）"
    );

    // 既知の制限: スナップショットの後から、同じ内容を main に保存しても、そのスナップショットの親には
    // 含まれないため除外されない（`rev-list --not --branches` は、ブランチ先端の木を辿らないため）。
    // 後から保存された内容を S が過大に数えることを、ここで固定しておく
    fs::write(repo.join("work.bin"), noise(9, BLOB)).expect("write");
    common::git_command(&repo, &["add", "-A"]);
    common::git_command(&repo, &["commit", "-m", "save the same content"]);
    let s3 = snapshot_only_bytes(&runner, &repo).expect("S");
    assert!(
        s3 >= s2 - 10_000,
        "既知の制限: 後から保存した内容は S から外れない: {s3}（前 {s2}）"
    );
}
