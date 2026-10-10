// 保存メモの提案に渡すテキストの差分（Ops::memo_text_diff）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 作業フォルダの未保存の変更が差分に反映されること、読み取りだけで作業フォルダ・インデックス・refs を変えないことを確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{ChangedKind, Identity, Ops, MEMO_TEXT_MAX_BYTES};
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

/// 作業フォルダ・インデックス・refs の状態を表す文字列（変わっていないことの確認に使う）
fn repo_state(repo: &Path) -> String {
    let index = run_git(repo, &["ls-files", "-s", "-z"]).1;
    let refs = run_git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]).1;
    let status = run_git(repo, &["status", "--porcelain=v2", "-z", "-uall"]).1;
    format!("{index}\n{refs}\n{status}")
}

#[test]
fn modified_text_shows_only_the_changed_lines() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "一行目\n二行目\n三行目\n")?;
    ops().save(&repo, "初回")?;
    write_test_file(&repo, "a.txt", "一行目\n二行目を変更\n三行目\n四行目\n")?;

    let diff = ops()
        .memo_text_diff(&repo, "a.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("テキストの差分が取れませんでした")?;
    assert_eq!(diff.added, 2);
    assert_eq!(diff.removed, 1);
    assert!(diff.text.contains("- 二行目\n"), "{}", diff.text);
    assert!(diff.text.contains("+ 二行目を変更\n"), "{}", diff.text);
    assert!(diff.text.contains("+ 四行目\n"), "{}", diff.text);
    // 変わらない行は本文に入らない
    assert!(!diff.text.contains("一行目"), "{}", diff.text);
    Ok(())
}

#[test]
fn new_untracked_text_is_all_added_and_nothing_is_written() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "x\n")?;
    ops().save(&repo, "初回")?;
    write_test_file(&repo, "新しい.txt", "あ\nい\nう\n")?;

    let before = repo_state(&repo);
    let diff = ops()
        .memo_text_diff(&repo, "新しい.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("テキストの差分が取れませんでした")?;
    assert_eq!((diff.added, diff.removed), (3, 0));
    assert_eq!(diff.text, "+ あ\n+ い\n+ う\n");
    // 読み取りの前後で、作業フォルダ・インデックス・refs は変わらない
    assert_eq!(before, repo_state(&repo));
    Ok(())
}

#[test]
fn the_list_of_changes_and_the_diff_agree_and_the_index_is_untouched() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "keep.txt", "残す\n")?;
    write_test_file(&repo, "gone.txt", "消す\n")?;
    ops().save(&repo, "初回")?;
    write_test_file(&repo, "keep.txt", "残す\n追加\n")?;
    std::fs::remove_file(repo.join("gone.txt"))?;

    let changes = ops().list_changes(&repo)?;
    let kinds: Vec<_> = {
        let mut v: Vec<_> = changes.iter().map(|c| (c.path.clone(), c.kind)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    assert_eq!(
        kinds,
        vec![
            ("gone.txt".to_string(), ChangedKind::Deleted),
            ("keep.txt".to_string(), ChangedKind::Modified),
        ]
    );

    let before = repo_state(&repo);
    let added = ops()
        .memo_text_diff(&repo, "keep.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("差分が取れませんでした")?;
    assert_eq!((added.added, added.removed), (1, 0));
    let deleted = ops()
        .memo_text_diff(&repo, "gone.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("削除された文書の差分が取れませんでした")?;
    assert_eq!((deleted.added, deleted.removed), (0, 1));
    assert_eq!(before, repo_state(&repo));
    // 削除されたファイルは作業フォルダに戻っていない
    assert!(!repo.join("gone.txt").exists());
    Ok(())
}

#[test]
fn binary_and_oversized_files_send_no_text() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "x\n")?;
    ops().save(&repo, "初回")?;
    std::fs::write(repo.join("bin.dat"), [0u8, 159, 146, 150, 0, 1])?;
    write_test_file(&repo, "big.txt", &"a\n".repeat(200_000))?;

    assert_eq!(
        ops().memo_text_diff(&repo, "bin.dat", MEMO_TEXT_MAX_BYTES)?,
        None
    );
    assert_eq!(
        ops().memo_text_diff(&repo, "big.txt", MEMO_TEXT_MAX_BYTES)?,
        None
    );
    Ok(())
}
