// 保存しないファイル（設計書 4.8）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// パターンの追加・重複・削除、追跡中の該当ファイルの検出、保存対象から外す操作（復元点・作業フォルダの保全）を確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Identity, IgnoreKind, Ops, OpsError, SaveOutcome};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn write(repo: &Path, rel: &str, content: &str) -> TestResult {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// 初期化して、いくつかのファイルを保存しておく（追跡中のファイルを作る）
fn repo_with_files(tmp: &Path, files: &[&str]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(&repo, None, &identity("tester"))?;
    for rel in files {
        write(&repo, rel, &format!("content of {rel}"))?;
    }
    ops().save(&repo, "first")?;
    Ok(repo)
}

fn gitignore(repo: &Path) -> String {
    std::fs::read_to_string(repo.join(".gitignore")).unwrap_or_default()
}

fn head_files(repo: &Path) -> Vec<String> {
    let (_, out, _) = run_git(repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    out.lines().map(str::to_string).collect()
}

/// 作業フォルダにあるファイル（`.git` の中は除く、`/` 区切り）
fn working_files(repo: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(repo, repo, &mut out);
    out.sort();
    out
}

fn restore_refs(repo: &Path, operation: &str) -> String {
    run_git(
        repo,
        &["for-each-ref", &format!("refs/hikae/backup/{operation}")],
    )
    .1
}

#[test]
fn add_writes_pattern_once_and_creates_a_restore_point() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.txt"])?;

    let pattern = ops().add_ignore_pattern(&repo, IgnoreKind::Extension, ".psd")?;
    assert_eq!(pattern, "*.psd");
    assert_eq!(gitignore(&repo), "*.psd\n");
    assert!(!restore_refs(&repo, "ignore-add").is_empty());

    // 同じパターンは重複して書かない（復元点も増やさない）
    let refs_before = restore_refs(&repo, "ignore-add");
    ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "psd")?;
    assert_eq!(gitignore(&repo), "*.psd\n");
    assert_eq!(restore_refs(&repo, "ignore-add"), refs_before);
    Ok(())
}

#[test]
fn add_builds_folder_and_file_patterns_and_keeps_existing_lines() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.txt"])?;
    write(&repo, ".gitignore", "# 手書き\n*.log")?;

    assert_eq!(
        ops().add_ignore_pattern(&repo, IgnoreKind::Folder, "build")?,
        "build/"
    );
    assert_eq!(
        ops().add_ignore_pattern(&repo, IgnoreKind::File, "docs/動画 1.mp4")?,
        "/docs/動画 1.mp4"
    );
    // コメントと既存の行は保つ。末尾の改行が無かったので補う
    assert_eq!(
        gitignore(&repo),
        "# 手書き\n*.log\nbuild/\n/docs/動画 1.mp4\n"
    );
    Ok(())
}

#[test]
fn add_keeps_crlf_line_endings() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.txt"])?;
    write(&repo, ".gitignore", "x\r\ny")?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Folder, "build")?;
    assert_eq!(gitignore(&repo), "x\r\ny\r\nbuild/\r\n");
    Ok(())
}

#[test]
fn add_rejects_paths_outside_the_project() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.txt"])?;
    assert!(matches!(
        ops().add_ignore_pattern(&repo, IgnoreKind::File, "../outside.txt"),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().add_ignore_pattern(&repo, IgnoreKind::File, ".git/config"),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "a/b"),
        Err(OpsError::InvalidInput(_))
    ));
    // 拒否したときは何も書かない
    assert!(!repo.join(".gitignore").exists());
    Ok(())
}

#[test]
fn remove_deletes_only_the_first_exact_line() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.txt"])?;
    write(&repo, ".gitignore", "*.log\n*.log\n# keep\n")?;

    ops().remove_ignore_pattern(&repo, "*.log")?;
    assert_eq!(gitignore(&repo), "*.log\n# keep\n");
    assert!(!restore_refs(&repo, "ignore-remove").is_empty());

    // 無い行、コメントは消せない
    assert!(matches!(
        ops().remove_ignore_pattern(&repo, "*.zip"),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().remove_ignore_pattern(&repo, "# keep"),
        Err(OpsError::InvalidInput(_))
    ));
    assert_eq!(gitignore(&repo), "*.log\n# keep\n");
    Ok(())
}

#[test]
fn list_counts_tracked_files_per_pattern() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(
        tmp.path(),
        &["a.psd", "b.psd", "dir/c.psd", "notes.txt", "build/x.bin"],
    )?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "psd")?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Folder, "build")?;
    write(
        &repo,
        ".gitignore",
        &format!("{}# 手書き\n!keep.psd\n", gitignore(&repo)),
    )?;

    let rules = ops().list_ignore_rules(&repo)?;
    let counts: Vec<(String, Option<u32>)> = rules
        .into_iter()
        .map(|r| (r.pattern, r.tracked_count))
        .collect();
    assert_eq!(
        counts,
        vec![
            ("*.psd".to_string(), Some(3)),
            ("build/".to_string(), Some(1)),
            ("!keep.psd".to_string(), None),
        ]
    );
    assert_eq!(
        ops().list_tracked_ignored_files(&repo)?,
        vec!["a.psd", "b.psd", "build/x.bin", "dir/c.psd"]
    );
    Ok(())
}

#[test]
fn untrack_removes_from_the_repository_but_keeps_working_files() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.psd", "b.psd", "dir/c.psd", "notes.txt"])?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "psd")?;
    let before = working_files(&repo);

    let outcome = ops().untrack_and_save(
        &repo,
        &["a.psd".to_string(), "dir/c.psd".to_string()],
        "保存しないファイルの設定を反映",
    )?;
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));

    // 追跡からは外れ、作業フォルダのファイルは残る（不変条件 6）
    let head = head_files(&repo);
    assert!(!head.contains(&"a.psd".to_string()));
    assert!(!head.contains(&"dir/c.psd".to_string()));
    assert!(head.contains(&"b.psd".to_string()));
    assert_eq!(working_files(&repo), before);
    assert_eq!(
        std::fs::read_to_string(repo.join("a.psd"))?,
        "content of a.psd"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("dir/c.psd"))?,
        "content of dir/c.psd"
    );

    // 復元点が作られ、保存の内容には .gitignore の変更も入る
    assert!(!restore_refs(&repo, "untrack").is_empty());
    assert_eq!(
        ops().list_tracked_ignored_files(&repo)?,
        vec!["b.psd".to_string()]
    );
    assert_eq!(gitignore(&repo), "*.psd\n");
    Ok(())
}

#[test]
fn untrack_refuses_files_that_are_not_tracked_and_ignored() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.psd", "notes.txt"])?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "psd")?;
    let head_before = get_head_commit(&repo);

    // 除外設定に当たらない追跡ファイル、存在しないファイル、プロジェクト外は拒否する
    for bad in ["notes.txt", "missing.psd", "../a.psd", "a.psd/../../x"] {
        assert!(
            matches!(
                ops().untrack_and_save(&repo, &[bad.to_string()], "x"),
                Err(OpsError::InvalidInput(_))
            ),
            "{bad} must be refused"
        );
    }
    assert!(matches!(
        ops().untrack_and_save(&repo, &[], "x"),
        Err(OpsError::InvalidInput(_))
    ));
    assert_eq!(get_head_commit(&repo), head_before);
    assert!(head_files(&repo).contains(&"a.psd".to_string()));
    Ok(())
}

#[test]
fn untracked_files_stay_out_of_later_saves() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.psd", "notes.txt"])?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Extension, "psd")?;
    ops().untrack_and_save(&repo, &["a.psd".to_string()], "x")?;

    // 外したあとに内容が変わっても、保存の対象には戻らない
    write(&repo, "a.psd", "changed")?;
    write(&repo, "notes.txt", "changed")?;
    ops().save(&repo, "later")?;
    assert!(!head_files(&repo).contains(&"a.psd".to_string()));
    assert!(repo.join("a.psd").exists());
    Ok(())
}

#[test]
fn saved_files_are_not_moved_by_untrack_of_other_files() -> TestResult {
    // 追跡解除は `rm --cached` のみ。ファイルの削除（作業フォルダ・インデックス以外の変更）は起きない
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_files(tmp.path(), &["a.psd", "dir/b.psd"])?;
    ops().add_ignore_pattern(&repo, IgnoreKind::Folder, "dir")?;
    ops().untrack_and_save(&repo, &["dir/b.psd".to_string()], "x")?;
    assert!(repo.join("dir/b.psd").exists());
    assert!(repo.join("a.psd").exists());
    assert!(head_files(&repo).contains(&"a.psd".to_string()));
    Ok(())
}
