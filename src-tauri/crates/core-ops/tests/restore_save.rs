// 元に戻した後の保存（Ops::restore_and_save / restore_file_and_save。設計書 4.2 手順 5）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。戻す前の復元点、続けて保存する経路（サイズ検査を含む）、
// 大きいファイルの確認が必要なときは保存せずに戻す操作だけが成功することを確かめる。
// 時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{AfterRestoreSave, Identity, Ops, RestoreFileOutcome, SizeLimits};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LIMITS: SizeLimits = SizeLimits {
    warn_bytes: 1024,
    block_bytes: 10 * 1024,
};

fn ops() -> Ops {
    // メモの末尾にトレーラーを付けない（メッセージを比べやすくする）
    Ops::new(GitRunner::from_path_env()).with_pc_name(None)
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

fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| format!("<{e}>"))
}

/// 1 回目の保存の後、編集して 2 回目を保存したリポジトリ。1 回目の保存の ID を返す
fn two_saves(tmp: &Path) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let repo = new_repo(tmp)?;
    write_test_file(&repo, "報告書.txt", "第1版")?;
    write_test_file(&repo, "メモ.txt", "メモ1")?;
    ops().save(&repo, "first")?;
    let first = get_head_commit(&repo).ok_or("no head")?;
    write_test_file(&repo, "報告書.txt", "第2版")?;
    ops().save(&repo, "second")?;
    Ok((repo, first))
}

#[test]
fn restore_then_save_records_the_restored_state_with_the_given_memo() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, first) = two_saves(tmp.path())?;
    let second = get_head_commit(&repo).ok_or("no head")?;
    // 戻す前の未保存の変更も、復元点（取り消し用）に入る
    write_test_file(&repo, "報告書.txt", "未保存の第3版")?;

    let done =
        ops().restore_and_save(&repo, &first, Some("10/4 18:02 の状態に戻しました"), LIMITS)?;

    assert_eq!(read(&repo, "報告書.txt"), "第1版");
    let AfterRestoreSave::Saved { commit } = done.save else {
        return Err(format!("expected a saved commit: {:?}", done.save).into());
    };
    assert_eq!(get_head_commit(&repo).as_deref(), Some(commit.as_str()));
    assert_eq!(
        git_out(&repo, &["log", "-1", "--format=%s"]).trim(),
        "10/4 18:02 の状態に戻しました"
    );
    // 履歴は消えない（戻す前の保存は祖先に残る）
    assert!(is_ancestor(&repo, &second, &commit));
    assert_eq!(read(&repo, "メモ.txt"), "メモ1");
    // 作業フォルダは保存済み（未保存の変更は残らない）
    assert!(!ops().sync_state(&repo)?.dirty);
    // 戻す前の作業状態は復元点に残っている
    let undo_ref = done.undo_ref.ok_or("undo_ref is missing")?;
    assert_eq!(
        git_out(
            &repo,
            &["cat-file", "-p", &format!("{undo_ref}:報告書.txt")]
        ),
        "未保存の第3版"
    );
    let backups = git_out(
        &repo,
        &["for-each-ref", "--format=%(refname)", "refs/hikae/backup/"],
    );
    assert!(backups.contains("refs/hikae/backup/restore/"));
    assert!(backups.contains("refs/hikae/backup/save/"));
    Ok(())
}

#[test]
fn without_a_memo_the_restore_is_left_unsaved() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, first) = two_saves(tmp.path())?;
    let head = get_head_commit(&repo);

    let done = ops().restore_and_save(&repo, &first, None, LIMITS)?;
    assert_eq!(done.save, AfterRestoreSave::NotRequested);
    assert_eq!(read(&repo, "報告書.txt"), "第1版");
    assert_eq!(get_head_commit(&repo), head);
    assert!(ops().sync_state(&repo)?.dirty);

    // 空のメモでは保存しない
    let done = ops().restore_and_save(&repo, &first, Some("  "), LIMITS)?;
    assert_eq!(done.save, AfterRestoreSave::NotRequested);
    assert_eq!(get_head_commit(&repo), head);
    Ok(())
}

#[test]
fn nothing_is_saved_when_the_restore_changes_nothing() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, _first) = two_saves(tmp.path())?;
    let head = get_head_commit(&repo).ok_or("no head")?;

    let done = ops().restore_and_save(&repo, &head, Some("戻しました"), LIMITS)?;
    assert_eq!(done.save, AfterRestoreSave::NothingToSave);
    assert_eq!(get_head_commit(&repo).as_deref(), Some(head.as_str()));
    Ok(())
}

#[test]
fn a_large_file_leaves_the_restore_done_but_the_save_to_the_normal_flow() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, first) = two_saves(tmp.path())?;
    let head = get_head_commit(&repo);
    // 戻す対象ではない未追跡の大きいファイル（このテストの警告閾値は 1KB）
    std::fs::write(repo.join("大きい.bin"), vec![3u8; 4096])?;

    let done = ops().restore_and_save(&repo, &first, Some("戻しました"), LIMITS)?;

    // 戻す操作は成功している。保存はしていない（HEAD は変わらず、変更は未保存のまま）
    assert_eq!(done.save, AfterRestoreSave::NeedsSizeDecision);
    assert!(done.undo_ref.is_some());
    assert_eq!(read(&repo, "報告書.txt"), "第1版");
    assert_eq!(get_head_commit(&repo), head);
    assert!(ops().sync_state(&repo)?.dirty);
    // 通常の保存の流れ（サイズ確認）では、同じ大きいファイルが確認を求める
    let outcome = ops().save_with(
        &repo,
        "手動の保存",
        &core_ops::SaveOptions {
            limits: LIMITS,
            ..core_ops::SaveOptions::default()
        },
    )?;
    assert!(matches!(
        outcome,
        core_ops::SaveOutcome::NeedsSizeDecision(_)
    ));
    Ok(())
}

#[test]
fn restoring_one_file_saves_only_after_it_was_restored() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, first) = two_saves(tmp.path())?;

    let done = ops().restore_file_and_save(
        &repo,
        &first,
        "報告書.txt",
        Some("報告書.txt を 10/4 の状態に戻しました"),
        LIMITS,
    )?;
    assert!(matches!(done.outcome, RestoreFileOutcome::Restored { .. }));
    assert!(matches!(done.save, AfterRestoreSave::Saved { .. }));
    assert_eq!(read(&repo, "報告書.txt"), "第1版");
    assert_eq!(
        git_out(&repo, &["log", "-1", "--format=%s"]).trim(),
        "報告書.txt を 10/4 の状態に戻しました"
    );

    // その時点に無いファイルは、戻せず、保存もしない
    write_test_file(&repo, "後から.txt", "x")?;
    ops().save(&repo, "third")?;
    let head = get_head_commit(&repo);
    let done =
        ops().restore_file_and_save(&repo, &first, "後から.txt", Some("戻しました"), LIMITS)?;
    assert_eq!(done.outcome, RestoreFileOutcome::NotInThatPoint);
    assert_eq!(done.save, AfterRestoreSave::NotRequested);
    assert_eq!(get_head_commit(&repo), head);
    assert_eq!(read(&repo, "後から.txt"), "x");
    Ok(())
}
