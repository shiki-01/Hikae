// 新規（未追跡）ファイルの差分（Ops::diff_file）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 「新規ファイルを選んでも右ペインの差分が空になる」不具合の再発防止が目的。時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    DiffLineKind, FileDiffOutcome, Identity, Ops, SaveOutcome, NEW_FILE_DIFF_MAX_BYTES,
};
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

fn save(repo: &Path, memo: &str) -> Result<String, Box<dyn std::error::Error>> {
    match ops().save(repo, memo)? {
        SaveOutcome::Saved { commit, .. } => Ok(commit),
        _ => Err("nothing saved".into()),
    }
}

fn diff_to_current(
    repo: &Path,
    from: &str,
    path: &str,
) -> Result<FileDiffOutcome, Box<dyn std::error::Error>> {
    Ok(ops().diff_file(repo, from, "current", Some(path))?)
}

#[test]
fn new_file_with_a_japanese_name_is_shown_as_all_added_lines_with_numbers() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "first.txt", "base\n")?;
    let base = save(&repo, "first")?;
    write_test_file(&repo, "資料/新しい 版.txt", "一行目\n二行目\r\n三行目")?;

    // 直前の保存と比べても、「いま」同士でも、同じ結果になる
    for from in [base.as_str(), "current"] {
        let FileDiffOutcome::NewFileLines(lines) =
            diff_to_current(&repo, from, "資料/新しい 版.txt")?
        else {
            return Err("expected the new file as lines".into());
        };
        let texts: Vec<_> = lines.iter().map(|l| l.content.as_str()).collect();
        assert_eq!(texts, vec!["一行目", "二行目", "三行目"]);
        assert!(lines.iter().all(|l| l.kind == DiffLineKind::Added));
        let numbers: Vec<_> = lines.iter().map(|l| l.line_number_new).collect();
        assert_eq!(numbers, vec![Some(1), Some(2), Some(3)]);
        assert!(lines.iter().all(|l| l.line_number_old.is_none()));
    }
    Ok(())
}

#[test]
fn new_file_in_a_repository_without_any_save_is_shown() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "memo.txt", "hello\n")?;

    let FileDiffOutcome::NewFileLines(lines) = diff_to_current(&repo, "current", "memo.txt")?
    else {
        return Err("expected the new file as lines".into());
    };
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].content, "hello");
    Ok(())
}

#[test]
fn empty_new_file_is_reported_as_empty() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "空.txt", "")?;

    assert_eq!(
        diff_to_current(&repo, "current", "空.txt")?,
        FileDiffOutcome::NewFileEmpty
    );
    Ok(())
}

#[test]
fn binary_new_file_returns_only_its_size() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(repo.join("図.png"), [0x89, b'P', b'N', b'G', 0, 1, 2, 3])?;

    let FileDiffOutcome::NewFileBinary {
        size,
        modified_unix,
    } = diff_to_current(&repo, "current", "図.png")?
    else {
        return Err("expected a binary new file".into());
    };
    assert_eq!(size, 8);
    assert!(modified_unix.is_some());
    Ok(())
}

#[test]
fn new_file_over_the_limit_is_omitted_but_the_limit_itself_is_shown() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    // 上限ちょうどは本文を出す（「超える」ものだけ省略）
    let at_limit = "a\n".repeat((NEW_FILE_DIFF_MAX_BYTES / 2) as usize);
    assert_eq!(at_limit.len() as u64, NEW_FILE_DIFF_MAX_BYTES);
    write_test_file(&repo, "ちょうど.txt", &at_limit)?;
    assert!(matches!(
        diff_to_current(&repo, "current", "ちょうど.txt")?,
        FileDiffOutcome::NewFileLines(_)
    ));

    write_test_file(&repo, "大きい.txt", &format!("{at_limit}b"))?;
    let FileDiffOutcome::NewFileTooLarge { size, .. } =
        diff_to_current(&repo, "current", "大きい.txt")?
    else {
        return Err("expected too large".into());
    };
    assert_eq!(size, NEW_FILE_DIFF_MAX_BYTES + 1);
    Ok(())
}

#[test]
fn tracked_files_still_use_the_normal_diff() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "one\ntwo\n")?;
    let base = save(&repo, "first")?;
    write_test_file(&repo, "a.txt", "one\nthree\n")?;

    let FileDiffOutcome::Lines(lines) = diff_to_current(&repo, &base, "a.txt")? else {
        return Err("expected the normal diff".into());
    };
    assert!(lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Added && l.content == "three"));
    assert!(lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Removed && l.content == "two"));
    Ok(())
}

#[test]
fn paths_outside_the_project_are_rejected() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(tmp.path().join("secret.txt"), "秘密\n")?;
    let outside = tmp.path().join("secret.txt");

    for bad in [
        "../secret.txt",
        "sub/../../secret.txt",
        outside.to_string_lossy().as_ref(),
        ".git/config",
        "",
    ] {
        assert!(
            ops()
                .diff_file(&repo, "current", "current", Some(bad))
                .is_err(),
            "{bad:?} must be rejected"
        );
    }
    Ok(())
}

#[test]
fn symlinks_are_not_followed() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "秘密の内容\n")?;

    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&secret, repo.join("link.txt")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&secret, repo.join("link.txt")).is_ok();
    if !linked {
        eprintln!("skip: symlink を作成できない環境");
        return Ok(());
    }

    // リンク先の本文は出さず、情報だけを返す
    assert!(matches!(
        diff_to_current(&repo, "current", "link.txt")?,
        FileDiffOutcome::NewFileBinary { .. }
    ));
    Ok(())
}
