//! 実リポジトリで生成した `status --porcelain=v2 -z --branch` の出力を解析する。

use core_git::{parse_status_v2, GitRunner, StatusCode, StatusKind};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// リポジトリのセットアップ用にだけ git を直接呼ぶテスト補助
fn git(repo: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn setup() -> TempDir {
    let t = TempDir::new().expect("tempdir");
    let r = t.path();
    git(r, &["init", "-b", "main"]);
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("user.name", "T"),
        ("user.email", "t@example.com"),
    ] {
        git(r, &["config", "--local", k, v]);
    }
    t
}

fn status(repo: &Path) -> core_git::StatusV2 {
    let out = GitRunner::from_path_env()
        .run_ok(repo, &["status", "--porcelain=v2", "-z", "--branch"])
        .expect("status");
    parse_status_v2(&out.stdout).expect("parse")
}

#[test]
fn parses_change_rename_untracked_and_japanese_names() {
    let t = setup();
    let r = t.path();
    fs::write(r.join("keep.txt"), "keep\n").expect("w");
    fs::write(r.join("gone.txt"), "gone\n").expect("w");
    fs::write(
        r.join("old name.txt"),
        "a long enough body for rename detection\nline2\nline3\n",
    )
    .expect("w");
    fs::write(r.join("日本語 ファイル.txt"), "jp\n").expect("w");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-m", "init"]);

    fs::write(r.join("keep.txt"), "changed\n").expect("w"); // 未ステージの変更
    fs::remove_file(r.join("gone.txt")).expect("rm"); // 未ステージの削除
    fs::rename(r.join("old name.txt"), r.join("new name.txt")).expect("mv");
    fs::write(r.join("日本語 ファイル.txt"), "jp2\n").expect("w");
    fs::write(r.join("新規 未追跡.txt"), "u\n").expect("w");
    git(r, &["add", "-A", "--", "new name.txt", "old name.txt"]);

    let s = status(r);
    assert_eq!(s.branch.head.as_deref(), Some("main"));
    let find = |p: &str| {
        s.entries
            .iter()
            .find(|e| e.path == p)
            .unwrap_or_else(|| panic!("{p} not found in {:?}", s.entries))
    };
    assert_eq!(
        find("keep.txt").kind,
        StatusKind::Change {
            index: StatusCode::Unmodified,
            worktree: StatusCode::Modified
        }
    );
    assert_eq!(
        find("gone.txt").kind,
        StatusKind::Change {
            index: StatusCode::Unmodified,
            worktree: StatusCode::Deleted
        }
    );
    assert!(matches!(
        &find("new name.txt").kind,
        StatusKind::Rename { original_path, copy: false, .. } if original_path == "old name.txt"
    ));
    assert!(matches!(
        find("日本語 ファイル.txt").kind,
        StatusKind::Change { .. }
    ));
    assert_eq!(find("新規 未追跡.txt").kind, StatusKind::Untracked);
}

#[test]
fn parses_unmerged_entries_of_a_real_conflict() {
    let t = setup();
    let r = t.path();
    fs::write(r.join("c 衝突.txt"), "base\n").expect("w");
    fs::write(r.join("d.txt"), "d\n").expect("w");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-m", "base"]);
    git(r, &["checkout", "-b", "other"]);
    fs::write(r.join("c 衝突.txt"), "theirs\n").expect("w");
    fs::remove_file(r.join("d.txt")).expect("rm");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-m", "other"]);
    git(r, &["checkout", "main"]);
    fs::write(r.join("c 衝突.txt"), "ours\n").expect("w");
    fs::write(r.join("d.txt"), "d changed\n").expect("w");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-m", "main"]);

    // 競合は終了コード 1（run_allow_codes で許容）
    GitRunner::from_path_env()
        .run_allow_codes(r, &["merge", "--no-edit", "other"], &[1])
        .expect("merge");

    let s = status(r);
    let xy = |p: &str| match &s.entries.iter().find(|e| e.path == p).expect("entry").kind {
        StatusKind::Unmerged { xy } => xy.clone(),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(xy("c 衝突.txt"), "UU");
    assert_eq!(xy("d.txt"), "UD"); // クラウド側（other）で削除、こちらで変更
    assert_eq!(s.unmerged().count(), 2);
}

#[test]
fn reports_ahead_and_behind() {
    let t = setup();
    let r = t.path();
    fs::write(r.join("a.txt"), "a\n").expect("w");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-m", "1"]);
    let bare = TempDir::new().expect("bare");
    git(bare.path(), &["init", "--bare", "-b", "main"]);
    git(
        r,
        &["remote", "add", "origin", &bare.path().to_string_lossy()],
    );
    git(r, &["push", "-u", "origin", "main"]);
    fs::write(r.join("a.txt"), "b\n").expect("w");
    git(r, &["commit", "-am", "2"]);
    let s = status(r);
    assert_eq!(s.branch.ab, Some((1, 0)));
    assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
}
