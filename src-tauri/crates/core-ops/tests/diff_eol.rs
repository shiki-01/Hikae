// 差分の取得（Ops::diff_with）で、最後の行の改行の有無・行末の CR だけの違いが「変更」に出ないことの統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{DiffLine, DiffLineKind, Identity, Ops};
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

fn changed(lines: &[DiffLine]) -> Vec<(DiffLineKind, String)> {
    lines
        .iter()
        .filter(|l| l.kind != DiffLineKind::Context)
        .map(|l| (l.kind, l.content.clone()))
        .collect()
}

#[test]
fn appending_after_a_last_line_without_a_newline_does_not_show_that_line_as_changed() -> TestResult
{
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    // 最後の行（「へへ」）に改行が無い状態で保存する
    write_test_file(&repo, "a.txt", "今日はいい天気ですね\nへへ")?;
    ops().save(&repo, "最初")?;

    // 最後の行の後に、空行と 1 行を足す（「へへ」の行は内容が同じで、改行だけが付く）
    write_test_file(
        &repo,
        "a.txt",
        "今日はいい天気ですね\nへへ\n\nTODO:明日はここを進める\n",
    )?;
    let diff = ops().diff_with(&repo, "HEAD", "current", Some("a.txt"))?;

    assert_eq!(
        changed(&diff),
        vec![
            (DiffLineKind::Added, String::new()),
            (DiffLineKind::Added, "TODO:明日はここを進める".to_string()),
        ],
        "「へへ」は変更として出ない: {diff:?}"
    );
    Ok(())
}

#[test]
fn a_line_that_really_changed_is_still_shown() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "今日はいい天気ですね\nへへ")?;
    ops().save(&repo, "最初")?;

    write_test_file(&repo, "a.txt", "今日はいい天気ですね\nふふ\n")?;
    let diff = ops().diff_with(&repo, "HEAD", "current", Some("a.txt"))?;
    let kinds = changed(&diff);
    assert!(
        kinds.contains(&(DiffLineKind::Removed, "へへ".to_string())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(DiffLineKind::Added, "ふふ".to_string())),
        "{kinds:?}"
    );
    Ok(())
}

#[test]
fn a_change_that_only_adds_the_final_newline_shows_no_difference() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "あ\nい")?;
    ops().save(&repo, "最初")?;

    write_test_file(&repo, "a.txt", "あ\nい\n")?;
    let diff = ops().diff_with(&repo, "HEAD", "current", Some("a.txt"))?;
    assert!(changed(&diff).is_empty(), "{diff:?}");
    Ok(())
}
