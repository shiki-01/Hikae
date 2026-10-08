//! 復元点の間引きの統合テスト。実リポジトリを使い、時刻は引数で渡す（実時間・sleep を使わない）。

mod common;

use core_git::GitRunner;
use core_safety::{
    create_backup_ref, create_snapshot, thin_restore_points, ThinPolicy, BACKUP_REF_PREFIX,
    SNAPSHOT_REF_PREFIX,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-10-08 12:00:00 UTC);

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let out = common::git_command(repo, args);
    assert!(out.status.success(), "git {args:?} 失敗");
    String::from_utf8(out.stdout).expect("UTF-8 でない出力")
}

fn setup() -> (TempDir, std::path::PathBuf) {
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
    fs::write(repo.join("a.txt"), "base\n").expect("write");
    common::git_command(&repo, &["add", "-A"]);
    common::git_command(&repo, &["commit", "-m", "init"]);
    (temp, repo)
}

/// 内容を変えてから、指定時刻のスナップショットを作る。作った ref 名を返す
fn snapshot_at(runner: &GitRunner, repo: &Path, branch: &str, at: OffsetDateTime) -> String {
    fs::write(repo.join("a.txt"), format!("edited at {at}\n")).expect("write");
    create_snapshot(runner, repo, branch, at)
        .expect("snapshot")
        .expect("snapshot should be created")
        .ref_name
}

fn all_refs(repo: &Path, pattern: &str) -> Vec<String> {
    git_stdout(repo, &["for-each-ref", "--format=%(refname)", pattern])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn thinning_keeps_retained_refs_and_never_touches_branches_or_tags() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();

    // 通常のブランチ・タグ・隠し名前空間の外の ref（間引きで変わってはいけない）
    common::git_command(&repo, &["branch", "feature/keep"]);
    common::git_command(&repo, &["tag", "v1"]);
    let head = git_stdout(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    common::git_command(&repo, &["update-ref", "refs/hikae/other/note", &head]);
    let outside_before = git_stdout(
        &repo,
        &[
            "for-each-ref",
            "refs/heads/",
            "refs/tags/",
            "refs/hikae/other/",
        ],
    );
    assert!(outside_before.contains("feature/keep") && outside_before.contains("v1"));

    // 自動保存: 直近 2 日（全件残る）、10 日前の同じ時間帯に 2 件、50 日前に 2 件（同じ日）、
    // 120 日前（保持期間超過）、別ブランチの古い 1 件
    let recent_1 = snapshot_at(&runner, &repo, "main", NOW - Duration::hours(1));
    let recent_2 = snapshot_at(&runner, &repo, "main", NOW - Duration::days(2));
    let ten_a = snapshot_at(&runner, &repo, "main", NOW - Duration::days(10));
    let ten_b = snapshot_at(
        &runner,
        &repo,
        "main",
        NOW - Duration::days(10) + Duration::minutes(5),
    );
    let fifty_a = snapshot_at(&runner, &repo, "main", NOW - Duration::days(50));
    let fifty_b = snapshot_at(
        &runner,
        &repo,
        "main",
        NOW - Duration::days(50) + Duration::hours(1),
    );
    let ancient = snapshot_at(&runner, &repo, "main", NOW - Duration::days(120));
    let other_branch_old = snapshot_at(&runner, &repo, "work", NOW - Duration::days(300));
    let protected_old = snapshot_at(&runner, &repo, "main", NOW - Duration::days(200));

    // 操作前の控え: 古い pull が 2 件（新しい側が最新として残る）、新しい restore
    let backup_old = create_backup_ref(&runner, &repo, "pull", NOW - Duration::days(200))
        .expect("backup")
        .expect("ref");
    let backup_newer_old = create_backup_ref(&runner, &repo, "pull", NOW - Duration::days(150))
        .expect("backup")
        .expect("ref");
    let backup_fresh = create_backup_ref(&runner, &repo, "restore", NOW - Duration::days(30))
        .expect("backup")
        .expect("ref");

    // 作業フォルダとインデックスの状態（間引きで変わってはいけない）
    let status_before = git_stdout(&repo, &["status", "--porcelain=v2"]);
    let index_before = common::read_index_file(&repo).expect("index");

    let protected: HashSet<String> = [protected_old.clone()].into();
    let report =
        thin_restore_points(&runner, &repo, NOW, &ThinPolicy::default(), &protected).expect("thin");
    assert!(report.failed.is_empty(), "{:?}", report.failed);

    let mut expected_deleted = vec![
        ten_a.clone(),
        fifty_a.clone(),
        ancient.clone(),
        backup_old.clone(),
    ];
    expected_deleted.sort();
    assert_eq!(report.deleted, expected_deleted);

    let remaining: HashSet<String> = all_refs(&repo, SNAPSHOT_REF_PREFIX)
        .into_iter()
        .chain(all_refs(&repo, BACKUP_REF_PREFIX))
        .collect();
    for kept in [
        &recent_1,
        &recent_2,
        &ten_b,
        &fifty_b,
        &other_branch_old,
        &protected_old,
        &backup_newer_old,
        &backup_fresh,
    ] {
        assert!(remaining.contains(kept), "削除してはいけない: {kept}");
    }
    for gone in &expected_deleted {
        assert!(!remaining.contains(gone), "削除されるはず: {gone}");
    }

    // 通常のブランチ・タグ・名前空間の外の ref は完全に不変
    let outside_after = git_stdout(
        &repo,
        &[
            "for-each-ref",
            "refs/heads/",
            "refs/tags/",
            "refs/hikae/other/",
        ],
    );
    assert_eq!(outside_before, outside_after);
    // 作業フォルダとインデックスにも触れていない
    assert_eq!(
        status_before,
        git_stdout(&repo, &["status", "--porcelain=v2"])
    );
    assert_eq!(
        index_before,
        common::read_index_file(&repo).expect("index after")
    );

    // 2 回目は何も削除しない
    let again = thin_restore_points(&runner, &repo, NOW, &ThinPolicy::default(), &protected)
        .expect("thin again");
    assert!(again.deleted.is_empty() && again.failed.is_empty());
}

#[test]
fn thinning_an_empty_namespace_is_a_no_op() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    let report = thin_restore_points(&runner, &repo, NOW, &ThinPolicy::default(), &HashSet::new())
        .expect("thin");
    assert!(report.deleted.is_empty() && report.failed.is_empty());
}

#[test]
fn the_newest_restore_point_survives_even_when_everything_is_expired() {
    let (_temp, repo) = setup();
    let runner = GitRunner::from_path_env();
    let older = snapshot_at(&runner, &repo, "main", NOW - Duration::days(400));
    let newest = snapshot_at(&runner, &repo, "main", NOW - Duration::days(300));
    let report = thin_restore_points(&runner, &repo, NOW, &ThinPolicy::default(), &HashSet::new())
        .expect("thin");
    assert_eq!(report.deleted, vec![older]);
    assert_eq!(all_refs(&repo, SNAPSHOT_REF_PREFIX), vec![newest]);
}
