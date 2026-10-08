// 差分（diff_with）と元に戻すプレビュー（restore_preview）の出力解析の統合テスト。
// 日本語・空白を含む名前や、`--` で始まる内容の行を正しく扱えることを確認する。実 git の実リポジトリを使う。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{DiffLineKind, Identity, Ops, SaveOutcome};
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

#[test]
fn restore_preview_keeps_japanese_and_spaced_names() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "第3章 まとめ.docx", "v1")?;
    write_test_file(&repo, "資料/古い 版.txt", "old")?;
    let first = save(&repo, "first")?;

    write_test_file(&repo, "第3章 まとめ.docx", "v2")?;
    std::fs::remove_file(repo.join("資料/古い 版.txt"))?;
    write_test_file(&repo, "新しい 版.txt", "new")?;
    save(&repo, "second")?;

    let preview = ops().restore_preview(&repo, &first)?;
    let modified: Vec<_> = preview.modified.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(modified, vec!["第3章 まとめ.docx"]);
    assert_eq!(preview.deleted, vec!["新しい 版.txt".to_string()]);
    assert_eq!(preview.created, vec!["資料/古い 版.txt".to_string()]);
    Ok(())
}

#[test]
fn diff_keeps_lines_that_start_with_dashes_or_pluses() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "メモ 1.txt", "keep\n-- removed comment\nend\n")?;
    let first = save(&repo, "first")?;
    write_test_file(&repo, "メモ 1.txt", "keep\n++ added line\nend\n")?;
    let second = save(&repo, "second")?;

    let lines = ops().diff_with(&repo, &first, &second, Some("メモ 1.txt"))?;
    let removed: Vec<_> = lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Removed)
        .map(|l| l.content.as_str())
        .collect();
    let added: Vec<_> = lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Added)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(removed, vec!["-- removed comment"]);
    assert_eq!(added, vec!["++ added line"]);
    // 行番号は 1 始まり。変更行は 2 行目
    let first_removed = lines
        .iter()
        .find(|l| l.kind == DiffLineKind::Removed)
        .ok_or("no removed line")?;
    assert_eq!(first_removed.line_number_old, Some(2));
    let first_added = lines
        .iter()
        .find(|l| l.kind == DiffLineKind::Added)
        .ok_or("no added line")?;
    assert_eq!(first_added.line_number_new, Some(2));
    Ok(())
}

#[test]
fn diff_against_current_compares_with_the_working_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a b.txt", "one\ntwo\n")?;
    let first = save(&repo, "first")?;
    write_test_file(&repo, "a b.txt", "one\n2\n")?;

    let lines = ops().diff_with(&repo, &first, "current", Some("a b.txt"))?;
    let changed: Vec<_> = lines
        .iter()
        .filter(|l| l.kind != DiffLineKind::Context)
        .map(|l| (l.kind, l.content.as_str()))
        .collect();
    assert_eq!(
        changed,
        vec![(DiffLineKind::Removed, "two"), (DiffLineKind::Added, "2")]
    );
    Ok(())
}
