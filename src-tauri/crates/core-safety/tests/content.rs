//! スナップショットの中身と、本物のインデックスの状態が保たれることの検証。

mod common;

use core_git::GitRunner;
use core_safety::create_snapshot;
use std::fs;
use tempfile::TempDir;
use time::macros::datetime;

fn git_stdout(repo: &std::path::Path, args: &[&str]) -> String {
    let out = common::git_command(repo, args);
    assert!(out.status.success(), "git {args:?} 失敗");
    String::from_utf8(out.stdout).expect("UTF-8 でない出力")
}

#[test]
fn snapshot_contains_worktree_state_and_keeps_staging() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    common::git_command(repo, &["init"]);
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("core.quotepath", "false"),
        ("user.email", "t@example.com"),
        ("user.name", "T"),
    ] {
        common::git_command(repo, &["config", "--local", k, v]);
    }
    fs::write(repo.join("a.txt"), "a0\n").expect("write");
    fs::write(repo.join("b.txt"), "b0\n").expect("write");
    fs::create_dir(repo.join("sub")).expect("mkdir");
    fs::write(repo.join("sub/c.txt"), "c0\n").expect("write");
    common::git_command(repo, &["add", "-A"]);
    common::git_command(repo, &["commit", "-m", "init"]);
    let head = git_stdout(repo, &["rev-parse", "HEAD"]).trim().to_string();

    // 未ステージの変更、ステージ済みの変更、削除、日本語名の未追跡、無視対象
    fs::write(repo.join("a.txt"), "a1\n").expect("write");
    fs::write(repo.join("b.txt"), "b1\n").expect("write");
    common::git_command(repo, &["add", "b.txt"]);
    fs::write(repo.join("b.txt"), "b2\n").expect("write"); // ステージ後にさらに変更
    fs::remove_file(repo.join("sub/c.txt")).expect("rm");
    fs::write(repo.join("新規 ファイル.txt"), "new\n").expect("write");
    fs::write(repo.join(".gitignore"), "*.log\n").expect("write");
    fs::write(repo.join("ignored.log"), "x\n").expect("write");

    let status_before = git_stdout(repo, &["status", "--porcelain=v2", "-z"]);
    let index_before = common::read_index_file(repo).expect("index");

    let runner = GitRunner::from_path_env();
    let now = datetime!(2026-10-07 14:30:45 UTC);
    let snap = create_snapshot(&runner, repo, "main", now)
        .expect("snapshot")
        .expect("Some");

    // 本物のインデックスと status が不変（b.txt はステージ済みのまま）
    assert_eq!(index_before, common::read_index_file(repo).expect("index"));
    assert_eq!(
        status_before,
        git_stdout(repo, &["status", "--porcelain=v2", "-z"])
    );
    // HEAD も動いていない
    assert_eq!(head, git_stdout(repo, &["rev-parse", "HEAD"]).trim());

    // commit の親は HEAD
    let parent = git_stdout(repo, &["rev-parse", &format!("{}^", snap.commit)]);
    assert_eq!(head, parent.trim());

    // ツリーの中身: 作業フォルダの最新状態（ステージ後の変更を含む）
    let tree = &snap.tree;
    let show = |path: &str| git_stdout(repo, &["show", &format!("{tree}:{path}")]);
    assert_eq!(show("a.txt"), "a1\n");
    assert_eq!(show("b.txt"), "b2\n");
    assert_eq!(show("新規 ファイル.txt"), "new\n");
    assert_eq!(show(".gitignore"), "*.log\n");

    let files = git_stdout(repo, &["ls-tree", "-r", "--name-only", tree]);
    assert!(!files.contains("sub/c.txt"), "削除したファイルが残っている");
    assert!(!files.contains("ignored.log"), "無視対象が含まれている");
}
