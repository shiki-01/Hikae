// 未保存の変更の一覧（Ops::list_changes）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 「ファイルを置いたのに一覧に出ない」不具合の再発防止が目的。時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{ChangedFile, ChangedKind, Identity, Ops};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(
        &repo,
        None,
        &Identity {
            name: "tester".to_string(),
            email: "tester@users.noreply.github.com".to_string(),
        },
    )?;
    Ok(repo)
}

/// パスで並べた (パス, 種類) の一覧
fn summary(changes: &[ChangedFile]) -> Vec<(String, ChangedKind)> {
    let mut v: Vec<_> = changes.iter().map(|c| (c.path.clone(), c.kind)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn save_all(repo: &Path, memo: &str) -> TestResult {
    ops().save(repo, memo)?;
    Ok(())
}

#[test]
fn new_file_in_an_empty_repository_is_listed_as_added() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "memo.txt", "hello")?;

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![("memo.txt".to_string(), ChangedKind::Added)]
    );
    assert!(!changes[0].conflicted);
    Ok(())
}

#[test]
fn files_in_a_new_folder_are_listed_one_by_one() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "資料/a.txt", "a")?;
    write_test_file(&repo, "資料/b.txt", "b")?;
    write_test_file(&repo, "資料/sub/c.txt", "c")?;

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![
            ("資料/a.txt".to_string(), ChangedKind::Added),
            ("資料/b.txt".to_string(), ChangedKind::Added),
            ("資料/sub/c.txt".to_string(), ChangedKind::Added),
        ]
    );
    Ok(())
}

#[test]
fn japanese_names_with_spaces_are_kept_intact() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "第3章 まとめ.docx", "x")?;
    write_test_file(&repo, "提出 用/最終 版 (2).xlsx", "y")?;

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![
            ("提出 用/最終 版 (2).xlsx".to_string(), ChangedKind::Added),
            ("第3章 まとめ.docx".to_string(), ChangedKind::Added),
        ]
    );
    Ok(())
}

#[test]
fn modified_and_deleted_saved_files_are_distinguished() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "keep.txt", "1")?;
    write_test_file(&repo, "edit me.txt", "1")?;
    write_test_file(&repo, "gone.txt", "1")?;
    save_all(&repo, "first")?;
    assert!(ops().list_changes(&repo)?.is_empty());

    write_test_file(&repo, "edit me.txt", "2")?;
    std::fs::remove_file(repo.join("gone.txt"))?;

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![
            ("edit me.txt".to_string(), ChangedKind::Modified),
            ("gone.txt".to_string(), ChangedKind::Deleted),
        ]
    );
    Ok(())
}

#[test]
fn a_staged_rename_is_listed_as_renamed_with_its_original_path() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(
        &repo,
        "旧 名前.txt",
        "same content for similarity\nline2\nline3\n",
    )?;
    save_all(&repo, "first")?;

    let (code, _, err) = run_git(&repo, &["mv", "旧 名前.txt", "新 名前.txt"]);
    assert_eq!(code, 0, "{err}");

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![("新 名前.txt".to_string(), ChangedKind::Renamed)]
    );
    assert_eq!(changes[0].original_path.as_deref(), Some("旧 名前.txt"));
    Ok(())
}

#[test]
fn ignored_files_are_not_listed() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, ".gitignore", "*.log\ntemp/\n")?;
    write_test_file(&repo, "debug.log", "x")?;
    write_test_file(&repo, "temp/a.txt", "x")?;
    write_test_file(&repo, "real.txt", "x")?;

    let changes = ops().list_changes(&repo)?;
    assert_eq!(
        summary(&changes),
        vec![
            (".gitignore".to_string(), ChangedKind::Added),
            ("real.txt".to_string(), ChangedKind::Added),
        ]
    );
    Ok(())
}

#[test]
fn no_changes_gives_an_empty_list_and_does_not_touch_the_working_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    assert!(ops().list_changes(&repo)?.is_empty());

    write_test_file(&repo, "a.txt", "1")?;
    save_all(&repo, "first")?;
    assert!(ops().list_changes(&repo)?.is_empty());

    // 一覧の取得は作業フォルダ・インデックスを変えない
    write_test_file(&repo, "b.txt", "2")?;
    let before = run_git(&repo, &["status", "--porcelain=v2", "-z"]).1;
    ops().list_changes(&repo)?;
    let after = run_git(&repo, &["status", "--porcelain=v2", "-z"]).1;
    assert_eq!(before, after);
    assert!(run_git(&repo, &["diff", "--cached", "--name-only"])
        .1
        .is_empty());
    Ok(())
}

#[test]
fn the_unsaved_flag_of_sync_state_agrees_with_the_list() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    assert!(!ops().sync_state(&repo)?.dirty);
    assert!(ops().list_changes(&repo)?.is_empty());

    write_test_file(&repo, "x/a.txt", "1")?;
    write_test_file(&repo, "x/b.txt", "1")?;
    let changes = ops().list_changes(&repo)?;
    assert_eq!(changes.len(), 2);
    assert!(ops().sync_state(&repo)?.dirty);

    save_all(&repo, "first")?;
    assert!(!ops().sync_state(&repo)?.dirty);
    assert!(ops().list_changes(&repo)?.is_empty());
    Ok(())
}

#[test]
fn files_in_conflict_are_marked() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "doc.txt", "base\n")?;
    save_all(&repo, "base")?;

    // 同じ行を別々に変更する 2 つの枝を作り、取り込みでぶつからせる
    let (code, _, err) = run_git(&repo, &["checkout", "-b", "other"]);
    assert_eq!(code, 0, "{err}");
    write_test_file(&repo, "doc.txt", "other\n")?;
    save_all(&repo, "other side")?;
    let (code, _, err) = run_git(&repo, &["checkout", "main"]);
    assert_eq!(code, 0, "{err}");
    write_test_file(&repo, "doc.txt", "mine\n")?;
    save_all(&repo, "main side")?;
    write_test_file(&repo, "plain.txt", "new")?;
    let (code, _, _) = run_git(&repo, &["merge", "other"]);
    assert_ne!(code, 0, "merge should conflict");

    let changes = ops().list_changes(&repo)?;
    let doc = changes
        .iter()
        .find(|c| c.path == "doc.txt")
        .ok_or("doc.txt missing")?;
    assert!(doc.conflicted);
    let plain = changes
        .iter()
        .find(|c| c.path == "plain.txt")
        .ok_or("plain.txt missing")?;
    assert!(!plain.conflicted);
    assert_eq!(plain.kind, ChangedKind::Added);
    Ok(())
}
