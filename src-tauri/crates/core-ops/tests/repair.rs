// 破損リポジトリの復旧のファイル操作（設計書 5.2 E10）の統合テスト。
// 一時ディレクトリに実ファイルを作る。モックは使わない。git は使わない。

use core_ops::{copy_back_unsaved, move_aside, restore_aside, OpsError};
use std::fs;
use std::path::Path;

const STAMP: &str = "20260101-120000";

/// 親フォルダへ書き込む（親フォルダは作る）
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read")
}

// ---- (a) move_aside ----

#[test]
fn move_aside_moves_folder_and_keeps_content() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("卒業論文");
    write(&project.join("第1章.txt"), "本文");
    write(&project.join("図/図1.txt"), "図");

    let aside = move_aside(&project, STAMP).expect("move");

    assert_eq!(aside, tmp.path().join(format!("卒業論文_退避_{STAMP}")));
    assert!(!project.exists(), "元の場所から消えている");
    assert_eq!(read(&aside.join("第1章.txt")), "本文");
    assert_eq!(read(&aside.join("図/図1.txt")), "図");
}

#[test]
fn move_aside_adds_numbered_suffix_on_collision() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");

    write(&project.join("a.txt"), "first");
    let first = move_aside(&project, STAMP).expect("first move");

    write(&project.join("a.txt"), "second");
    let second = move_aside(&project, STAMP).expect("second move");

    write(&project.join("a.txt"), "third");
    let third = move_aside(&project, STAMP).expect("third move");

    assert_eq!(
        first.file_name().unwrap(),
        format!("proj_退避_{STAMP}").as_str()
    );
    assert_eq!(
        second.file_name().unwrap(),
        format!("proj_退避_{STAMP}-1").as_str()
    );
    assert_eq!(
        third.file_name().unwrap(),
        format!("proj_退避_{STAMP}-2").as_str()
    );
    // 既存の退避フォルダの内容は上書きされない
    assert_eq!(read(&first.join("a.txt")), "first");
    assert_eq!(read(&second.join("a.txt")), "second");
    assert_eq!(read(&third.join("a.txt")), "third");
}

#[test]
fn move_aside_rejects_malformed_timestamp() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");
    write(&project.join("a.txt"), "x");

    for bad in ["20260101120000", "2026-01-01-12", "20260101-12000a", ""] {
        let err = move_aside(&project, bad).expect_err("must fail");
        assert!(matches!(err, OpsError::InvalidInput(_)), "{bad}: {err:?}");
    }
    // 何も動いていない
    assert_eq!(read(&project.join("a.txt")), "x");
}

// ---- (b) restore_aside ----

#[test]
fn restore_aside_puts_folder_back() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");
    write(&project.join("a.txt"), "内容");
    let aside = move_aside(&project, STAMP).expect("move");

    restore_aside(&aside, &project).expect("restore");

    assert!(!aside.exists(), "退避フォルダは名前が戻って無くなる");
    assert_eq!(read(&project.join("a.txt")), "内容");
}

#[test]
fn restore_aside_parks_existing_partial_clone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");
    write(&project.join("a.txt"), "元の内容");
    let aside = move_aside(&project, STAMP).expect("move");

    // できかけの clone が元の名前にある
    write(&project.join("partial.txt"), "取得途中");

    restore_aside(&aside, &project).expect("restore");

    let parked = tmp.path().join(format!("proj_取得失敗_{STAMP}"));
    assert_eq!(
        read(&parked.join("partial.txt")),
        "取得途中",
        "できかけは退けられている"
    );
    assert_eq!(
        read(&project.join("a.txt")),
        "元の内容",
        "元の内容が戻っている"
    );
    assert!(!project.join("partial.txt").exists());
    assert!(!aside.exists());
}

#[test]
fn restore_aside_numbers_parked_name_and_keeps_suffix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("proj");
    write(&project.join("a.txt"), "元");
    // 2 回目の退避（名前に -1 が付く）
    let aside_first = move_aside(&project, STAMP).expect("first");
    write(&project.join("a.txt"), "元2");
    let aside = move_aside(&project, STAMP).expect("second");
    assert!(aside.ends_with(format!("proj_退避_{STAMP}-1")));

    // 既に取得失敗のフォルダがある
    write(
        &tmp.path()
            .join(format!("proj_取得失敗_{STAMP}-1"))
            .join("x.txt"),
        "古い",
    );
    write(&project.join("new.txt"), "できかけ");

    restore_aside(&aside, &project).expect("restore");

    // 日時の後ろ（-1）を引き継ぎ、衝突時は -N を付けて退ける
    let parked = tmp.path().join(format!("proj_取得失敗_{STAMP}-1-1"));
    assert_eq!(read(&parked.join("new.txt")), "できかけ");
    assert_eq!(
        read(
            &tmp.path()
                .join(format!("proj_取得失敗_{STAMP}-1"))
                .join("x.txt")
        ),
        "古い"
    );
    assert_eq!(read(&project.join("a.txt")), "元2");
    assert!(aside_first.exists(), "別の退避フォルダには触れない");
}

// ---- (c) copy_back_unsaved ----

#[test]
fn copy_back_copies_new_and_different_and_skips_same() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("proj_退避_x");
    let new_dir = tmp.path().join("proj");

    // 退避フォルダ（利用者の手元の内容）
    write(&aside.join("same.txt"), "同じ");
    write(&aside.join("diff.txt"), "手元の版");
    write(&aside.join("only_local.txt"), "新規");
    // 新しい作業フォルダ（clone の結果）
    write(&new_dir.join("same.txt"), "同じ");
    write(&new_dir.join("diff.txt"), "clone の版（長さが違う）");
    write(&new_dir.join("only_clone.txt"), "消されない");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert_eq!(report.copied, 2, "{report:?}");
    assert_eq!(report.skipped_same, 1, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(read(&new_dir.join("only_local.txt")), "新規");
    assert_eq!(
        read(&new_dir.join("diff.txt")),
        "手元の版",
        "利用者の手元の状態を優先する"
    );
    assert_eq!(
        read(&new_dir.join("only_clone.txt")),
        "消されない",
        "新しい側のファイルは削除しない"
    );
    // 退避フォルダは変更されない
    assert_eq!(read(&aside.join("diff.txt")), "手元の版");
    assert_eq!(read(&aside.join("same.txt")), "同じ");
}

#[test]
fn copy_back_same_size_different_bytes_is_copied() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    write(&aside.join("f.bin"), "abcdef");
    write(&new_dir.join("f.bin"), "abcdeX");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert_eq!(report.copied, 1);
    assert_eq!(read(&new_dir.join("f.bin")), "abcdef");
}

#[test]
fn copy_back_excludes_root_git_folder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    // ルート直下の .git（フォルダ）
    write(&aside.join(".git/config"), "[core]");
    write(&aside.join(".git/objects/x"), "obj");
    // ネストした .git は除外しない
    write(&aside.join("sub/.git"), "nested");
    write(&aside.join("README.md"), "説明");
    fs::create_dir_all(&new_dir).expect("mkdir");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert!(
        !new_dir.join(".git").exists(),
        "ルートの .git フォルダは含めない"
    );
    assert_eq!(read(&new_dir.join("sub/.git")), "nested");
    assert_eq!(read(&new_dir.join("README.md")), "説明");
    assert_eq!(report.copied, 2, "{report:?}");
    assert!(report.failed.is_empty());
}

#[test]
fn copy_back_excludes_root_git_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    // ルート直下の .git（ファイル。worktree の場合）
    write(&aside.join(".git"), "gitdir: elsewhere");
    write(&aside.join("README.md"), "説明");
    fs::create_dir_all(&new_dir).expect("mkdir");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert!(
        !new_dir.join(".git").exists(),
        "ルートの .git ファイルは含めない"
    );
    assert_eq!(report.copied, 1, "{report:?}");
}

#[test]
fn copy_back_handles_nested_and_japanese_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    write(&aside.join("資料/2026年度/第3章 結論.docx"), "docx の中身");
    write(&aside.join("資料/2026年度/深い/奥.txt"), "奥");
    fs::create_dir_all(&new_dir).expect("mkdir");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert_eq!(report.copied, 2, "{report:?}");
    assert_eq!(
        read(&new_dir.join("資料/2026年度/第3章 結論.docx")),
        "docx の中身"
    );
    assert_eq!(read(&new_dir.join("資料/2026年度/深い/奥.txt")), "奥");
}

#[test]
fn copy_back_reports_failed_and_continues() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    write(&aside.join("sub/x.txt"), "x");
    write(&aside.join("ok.txt"), "ok");
    // new_dir の sub がフォルダではなくファイルなので、sub/x.txt は作れない
    write(&new_dir.join("sub"), "ファイル");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert_eq!(report.failed, vec!["sub/x.txt".to_string()], "{report:?}");
    assert_eq!(report.copied, 1);
    assert_eq!(read(&new_dir.join("ok.txt")), "ok", "失敗しても続行する");
    assert_eq!(
        read(&aside.join("sub/x.txt")),
        "x",
        "退避フォルダに残っている"
    );
}

#[cfg(unix)]
#[test]
fn copy_back_ignores_symlinks() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let aside = tmp.path().join("a");
    let new_dir = tmp.path().join("b");
    write(&tmp.path().join("target.txt"), "外の内容");
    write(&aside.join("real.txt"), "本物");
    std::os::unix::fs::symlink(tmp.path().join("target.txt"), aside.join("link.txt"))
        .expect("symlink");
    fs::create_dir_all(&new_dir).expect("mkdir");

    let report = copy_back_unsaved(&aside, &new_dir).expect("copy");

    assert_eq!(report.copied, 1, "{report:?}");
    assert!(report.failed.is_empty());
    assert!(
        !new_dir.join("link.txt").exists(),
        "リンクは追わず、コピーもしない"
    );
}

#[test]
fn copy_back_missing_aside_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err =
        copy_back_unsaved(&tmp.path().join("無い"), &tmp.path().join("b")).expect_err("must fail");
    assert!(matches!(err, OpsError::Io(_)), "{err:?}");
}
