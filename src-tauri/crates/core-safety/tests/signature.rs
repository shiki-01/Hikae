//! スナップショットの作者（未ログインは固定の `Hikae`、ログイン済みは実ユーザー）の検証。

mod common;

use core_git::GitRunner;
use core_safety::{create_restore_point, create_restore_point_as, create_snapshot_as, Signature};
use std::fs;
use tempfile::TempDir;
use time::macros::datetime;

fn git_stdout(repo: &std::path::Path, args: &[&str]) -> String {
    let out = common::git_command(repo, args);
    assert!(out.status.success(), "git {args:?} 失敗");
    String::from_utf8(out.stdout).expect("UTF-8 でない出力")
}

fn init_repo(repo: &std::path::Path) {
    common::git_command(repo, &["init"]);
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("user.email", "t@example.com"),
        ("user.name", "T"),
    ] {
        common::git_command(repo, &["config", "--local", k, v]);
    }
    fs::write(repo.join("a.txt"), "a0\n").expect("write");
    common::git_command(repo, &["add", "-A"]);
    common::git_command(repo, &["commit", "-m", "init"]);
}

fn author_of(repo: &std::path::Path, commit: &str) -> String {
    git_stdout(repo, &["log", "-1", "--format=%an <%ae>", commit])
        .trim()
        .to_string()
}

#[test]
fn snapshot_author_is_the_given_user_and_defaults_to_hikae() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    // 未ログイン: 固定の署名
    fs::write(repo.join("a.txt"), "a1\n").expect("write");
    let anon = create_restore_point(
        &runner,
        repo,
        "main",
        "save",
        datetime!(2026-10-07 14:30:45 UTC),
    )
    .expect("restore point")
    .snapshot
    .expect("snapshot");
    assert_eq!(author_of(repo, &anon.commit), "Hikae <hikae@localhost>");

    // ログイン済み: 実ユーザー（GitHub の noreply アドレス）
    fs::write(repo.join("a.txt"), "a2\n").expect("write");
    let me = Signature::new("octocat", "583231+octocat@users.noreply.github.com");
    let signed = create_restore_point_as(
        &runner,
        repo,
        "main",
        "save",
        datetime!(2026-10-07 14:31:45 UTC),
        &me,
    )
    .expect("restore point")
    .snapshot
    .expect("snapshot");
    assert_eq!(
        author_of(repo, &signed.commit),
        "octocat <583231+octocat@users.noreply.github.com>"
    );
    // 作者も committer も実ユーザー（commit-tree は同じ user.* を使う）
    assert_eq!(
        git_stdout(repo, &["log", "-1", "--format=%cn", &signed.commit]).trim(),
        "octocat"
    );
}

#[test]
fn snapshot_with_an_unusable_signature_falls_back_to_hikae() {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path();
    init_repo(repo);
    let runner = GitRunner::from_path_env();

    fs::write(repo.join("a.txt"), "a1\n").expect("write");
    let snap = create_snapshot_as(
        &runner,
        repo,
        "main",
        datetime!(2026-10-07 14:30:45 UTC),
        &Signature::new("", "x@example.com"),
    )
    .expect("snapshot")
    .expect("Some");
    assert_eq!(author_of(repo, &snap.commit), "Hikae <hikae@localhost>");
}
