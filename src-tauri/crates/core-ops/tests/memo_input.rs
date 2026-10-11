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

/// Shift_JIS の文字列のバイト列（テスト用）
fn sjis(text: &str) -> Vec<u8> {
    let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!had_errors, "テスト用の文字列が Shift_JIS にない");
    bytes.into_owned()
}

#[test]
fn shift_jis_tracked_text_keeps_its_lines_and_counts() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(repo.join("日記.txt"), sjis("今日は晴れ\r\n"))?;
    ops().save(&repo, "初回")?;
    std::fs::write(
        repo.join("日記.txt"),
        sjis("今日は晴れ\r\n明日は雨の予定\r\n"),
    )?;

    let before = repo_state(&repo);
    let diff = ops()
        .memo_text_diff(&repo, "日記.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("Shift_JIS のテキストの差分が取れませんでした")?;
    assert_eq!((diff.added, diff.removed), (1, 0));
    // 文字化けせず、追加された行の本文が入る（改行は CRLF でも含まない）
    assert_eq!(diff.text, "+ 明日は雨の予定\n");
    assert_eq!(before, repo_state(&repo));
    Ok(())
}

#[test]
fn shift_jis_new_file_is_all_added() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "x\n")?;
    ops().save(&repo, "初回")?;
    std::fs::write(repo.join("メモ.txt"), sjis("牛乳を買う\n卵も買う\n"))?;

    let diff = ops()
        .memo_text_diff(&repo, "メモ.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("Shift_JIS の新規ファイルの差分が取れませんでした")?;
    assert_eq!((diff.added, diff.removed), (2, 0));
    assert_eq!(diff.text, "+ 牛乳を買う\n+ 卵も買う\n");
    Ok(())
}

#[test]
fn utf8_bom_is_not_sent_and_crlf_is_not_kept() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "x\n")?;
    ops().save(&repo, "初回")?;
    // Windows のメモ帳が書く BOM 付き UTF-8 の新規ファイル（CRLF）
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice("牛乳を買う\r\n".as_bytes());
    std::fs::write(repo.join("新規 テキスト ドキュメント.txt"), bytes)?;

    let diff = ops()
        .memo_text_diff(&repo, "新規 テキスト ドキュメント.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("差分が取れませんでした")?;
    assert_eq!(diff.text, "+ 牛乳を買う\n");
    assert!(!diff.text.contains('\u{feff}'));
    assert!(!diff.text.contains('\r'));
    Ok(())
}

#[test]
fn tracked_crlf_text_has_no_carriage_return_in_the_body() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(repo.join("a.txt"), "一行目\r\n二行目\r\n")?;
    ops().save(&repo, "初回")?;
    std::fs::write(repo.join("a.txt"), "一行目\r\n二行目\r\n追加\r\n")?;

    let diff = ops()
        .memo_text_diff(&repo, "a.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("差分が取れませんでした")?;
    assert_eq!((diff.added, diff.removed), (1, 0));
    assert_eq!(diff.text, "+ 追加\n");
    Ok(())
}

#[test]
fn text_that_is_neither_utf8_nor_shift_jis_sends_no_text() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "x\n")?;
    ops().save(&repo, "初回")?;
    // UTF-8 としても Shift_JIS としても読めない並び（0x81 の後ろに 0x20 は来ない）
    std::fs::write(repo.join("壊れた.txt"), [0x81u8, 0x20, 0x0A])?;
    assert_eq!(
        ops().memo_text_diff(&repo, "壊れた.txt", MEMO_TEXT_MAX_BYTES)?,
        None
    );
    Ok(())
}

#[test]
fn a_file_changed_from_utf8_to_shift_jis_is_read_line_by_line() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    // 保存時点は UTF-8。作業フォルダでは Shift_JIS に変えられた（差分の出力に両方の文字コードが混ざる）
    std::fs::write(repo.join("a.txt"), "既存の行です。\n")?;
    ops().save(&repo, "初回")?;
    std::fs::write(repo.join("a.txt"), sjis("既存の行です。\n追加の行\n"))?;

    let diff = ops()
        .memo_text_diff(&repo, "a.txt", MEMO_TEXT_MAX_BYTES)?
        .ok_or("差分が取れませんでした")?;
    assert_eq!((diff.added, diff.removed), (2, 1));
    assert_eq!(
        diff.text,
        "- 既存の行です。\n+ 既存の行です。\n+ 追加の行\n"
    );
    Ok(())
}
