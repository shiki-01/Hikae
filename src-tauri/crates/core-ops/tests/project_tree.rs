// プロジェクトフォルダ全体のファイル一覧（Ops::project_tree）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{ChangedKind, Identity, Ops, ProjectTree, PROJECT_TREE_MAX_ENTRIES};
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

fn paths(tree: &ProjectTree) -> Vec<&str> {
    tree.entries.iter().map(|e| e.path.as_str()).collect()
}

fn kind_of(tree: &ProjectTree, path: &str) -> Option<ChangedKind> {
    tree.entries
        .iter()
        .find(|e| e.path == path)
        .and_then(|e| e.change)
}

#[test]
fn lists_tracked_and_untracked_files_with_japanese_names_and_deep_folders() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "第3章 まとめ.docx", "v1")?;
    write_test_file(&repo, "a/b/c/d/e/f/深い.txt", "deep")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "資料/新しい.txt", "new")?;

    let tree = ops().project_tree(&repo, PROJECT_TREE_MAX_ENTRIES)?;
    assert!(!tree.truncated);
    assert_eq!(
        paths(&tree),
        vec![
            "a/b/c/d/e/f/深い.txt",
            "第3章 まとめ.docx",
            "資料/新しい.txt"
        ]
    );
    // サイズと更新日時が付く
    let deep = &tree.entries[0];
    assert_eq!(deep.size, Some(4));
    assert!(deep.modified_unix.is_some());
    Ok(())
}

#[test]
fn files_ignored_by_gitignore_and_the_git_folder_are_not_listed() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, ".gitignore", "*.log\nbuild/\n")?;
    write_test_file(&repo, "keep.txt", "k")?;
    write_test_file(&repo, "debug.log", "ignored")?;
    write_test_file(&repo, "build/out.bin", "ignored")?;
    write_test_file(&repo, "sub/trace.log", "ignored")?;

    let tree = ops().project_tree(&repo, PROJECT_TREE_MAX_ENTRIES)?;
    assert_eq!(paths(&tree), vec![".gitignore", "keep.txt"]);
    assert!(tree.entries.iter().all(|e| !e.path.starts_with(".git/")));
    Ok(())
}

#[test]
fn each_file_carries_its_kind_of_change() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "same.txt", "same")?;
    write_test_file(&repo, "edit.txt", "v1")?;
    write_test_file(&repo, "gone.txt", "bye")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "edit.txt", "v2")?;
    std::fs::remove_file(repo.join("gone.txt"))?;
    write_test_file(&repo, "added.txt", "new")?;

    let tree = ops().project_tree(&repo, PROJECT_TREE_MAX_ENTRIES)?;
    assert_eq!(kind_of(&tree, "same.txt"), None);
    assert_eq!(kind_of(&tree, "edit.txt"), Some(ChangedKind::Modified));
    assert_eq!(kind_of(&tree, "added.txt"), Some(ChangedKind::Added));
    // 削除されたファイルも一覧に残り、サイズは無い
    assert_eq!(kind_of(&tree, "gone.txt"), Some(ChangedKind::Deleted));
    let gone = tree.entries.iter().find(|e| e.path == "gone.txt");
    assert_eq!(gone.and_then(|e| e.size), None);
    Ok(())
}

#[test]
fn stops_at_the_limit_and_keeps_changed_files() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    for i in 0..30 {
        write_test_file(&repo, &format!("files/{i:02}.txt"), "x")?;
    }
    ops().save(&repo, "first")?;
    // 並び順では最後になるファイルを変更する
    write_test_file(&repo, "files/29.txt", "changed")?;
    write_test_file(&repo, "zzz-new.txt", "new")?;

    let tree = ops().project_tree(&repo, 10)?;
    assert!(tree.truncated);
    assert_eq!(tree.entries.len(), 10);
    assert_eq!(kind_of(&tree, "files/29.txt"), Some(ChangedKind::Modified));
    assert_eq!(kind_of(&tree, "zzz-new.txt"), Some(ChangedKind::Added));

    // 上限ちょうどなら打ち切りではない
    let all = ops().project_tree(&repo, 31)?;
    assert!(!all.truncated);
    assert_eq!(all.entries.len(), 31);
    Ok(())
}

#[test]
fn nested_repositories_are_not_listed_as_files() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "top.txt", "t")?;
    let nested = repo.join("nested");
    std::fs::create_dir_all(&nested)?;
    run_git(&nested, &["init", "-b", "main"]);
    write_test_file(&repo, "nested/inner.txt", "i")?;

    let tree = ops().project_tree(&repo, PROJECT_TREE_MAX_ENTRIES)?;
    assert!(paths(&tree).contains(&"top.txt"));
    assert!(tree.entries.iter().all(|e| !e.path.contains(".git")));
    assert!(tree.entries.iter().all(|e| !e.path.ends_with('/')));
    Ok(())
}

#[test]
fn an_empty_project_has_an_empty_tree() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let tree = ops().project_tree(&repo, PROJECT_TREE_MAX_ENTRIES)?;
    assert!(tree.entries.is_empty());
    assert!(!tree.truncated);
    Ok(())
}
