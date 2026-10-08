//! 自動保存のスナップショットで、指定したファイルを含めない（大きいファイルの除外）ことの検証。
//! 作業フォルダとインデックスを変更しないことも、実リポジトリで確かめる。

mod common;

use core_git::GitRunner;
use core_safety::{create_snapshot_excluding, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use time::macros::datetime;

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let out = common::git_command(repo, args);
    assert!(out.status.success(), "git {args:?} 失敗");
    String::from_utf8(out.stdout).expect("UTF-8 でない出力")
}

fn init_repo(repo: &Path) {
    common::git_command(repo, &["init", "-b", "main"]);
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("user.email", "t@example.com"),
        ("user.name", "T"),
    ] {
        common::git_command(repo, &["config", "--local", k, v]);
    }
    fs::write(repo.join("a.txt"), "a0\n").expect("write");
    fs::write(repo.join("big.bin"), "old big\n").expect("write");
    common::git_command(repo, &["add", "-A"]);
    common::git_command(repo, &["commit", "-m", "init"]);
}

fn files_in(repo: &Path, commit: &str) -> Vec<String> {
    let mut v: Vec<String> = git_stdout(repo, &["ls-tree", "-r", "--name-only", commit])
        .lines()
        .map(str::to_string)
        .collect();
    v.sort();
    v
}

fn show(repo: &Path, spec: &str) -> String {
    git_stdout(repo, &["show", spec])
}

#[test]
fn excluded_files_keep_the_head_version_and_new_ones_are_left_out() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    // 追跡済みの大きいファイルの変更、未追跡の大きいファイル、通常の変更
    fs::write(repo.join("big.bin"), "new big content\n").expect("write");
    fs::write(repo.join("新規 動画.mp4"), "video\n").expect("write");
    fs::write(repo.join("a.txt"), "a1\n").expect("write");

    let working_before = common::hash_working_tree(repo).expect("hash");
    let index_before = common::read_index_file(repo).expect("index");

    let snap = create_snapshot_excluding(
        &runner,
        repo,
        "main",
        datetime!(2026-10-08 09:00:00 UTC),
        &Signature::default(),
        &["big.bin".to_string(), "新規 動画.mp4".to_string()],
    )
    .expect("snapshot")
    .expect("Some");

    // 通常の変更は入り、除外したファイルは HEAD の版のまま（未追跡は入らない）
    assert_eq!(files_in(repo, &snap.commit), vec!["a.txt", "big.bin"]);
    assert_eq!(show(repo, &format!("{}:a.txt", snap.commit)), "a1\n");
    assert_eq!(show(repo, &format!("{}:big.bin", snap.commit)), "old big\n");

    // 作業フォルダ・インデックスは一切変わらない（不変条件 4）
    assert_eq!(
        common::hash_working_tree(repo).expect("hash"),
        working_before
    );
    assert_eq!(common::read_index_file(repo).expect("index"), index_before);
    assert!(common::check_no_index_lock(repo));
    assert!(common::check_no_temp_index(repo));
    // 作った ref は refs/hikae/ の中だけ
    assert!(snap.ref_name.starts_with("refs/hikae/snapshots/main/"));
}

#[test]
fn when_only_excluded_files_changed_no_snapshot_is_made() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    fs::write(repo.join("big.bin"), "changed but excluded\n").expect("write");
    let snap = create_snapshot_excluding(
        &runner,
        repo,
        "main",
        datetime!(2026-10-08 09:00:00 UTC),
        &Signature::default(),
        &["big.bin".to_string()],
    )
    .expect("snapshot");
    // HEAD と同じ内容になるため作らない
    assert!(snap.is_none());
}

#[test]
fn identical_content_is_not_snapshotted_twice() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    fs::write(repo.join("a.txt"), "a1\n").expect("write");
    let first = create_snapshot_excluding(
        &runner,
        repo,
        "main",
        datetime!(2026-10-08 09:00:00 UTC),
        &Signature::default(),
        &[],
    )
    .expect("snapshot");
    assert!(first.is_some());
    let second = create_snapshot_excluding(
        &runner,
        repo,
        "main",
        datetime!(2026-10-08 09:05:00 UTC),
        &Signature::default(),
        &[],
    )
    .expect("snapshot");
    assert!(second.is_none());
}

#[test]
fn pathspec_special_characters_in_a_file_name_are_taken_literally() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    // glob として解釈されると a.bin まで除外される名前
    fs::write(repo.join("[ab].bin"), "special\n").expect("write");
    fs::write(repo.join("a.bin"), "plain\n").expect("write");
    let snap = create_snapshot_excluding(
        &runner,
        repo,
        "main",
        datetime!(2026-10-08 09:00:00 UTC),
        &Signature::default(),
        &["[ab].bin".to_string()],
    )
    .expect("snapshot")
    .expect("Some");
    assert_eq!(
        files_in(repo, &snap.commit),
        vec!["a.bin", "a.txt", "big.bin"]
    );
}
