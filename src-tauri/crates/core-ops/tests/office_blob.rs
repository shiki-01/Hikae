// Office 文書（docx・xlsx）の読み取り（Ops::read_office、設計書 3.3 S4）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。読み取りのみであることを、作業フォルダ・インデックス・refs の変化で確かめる。
// 中身は Office として正しくなくてよい（読み取り側は拡張子と上限だけを見る）。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Identity, OfficeFile, Ops, OpsError, SaveOutcome};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 版ごとに違う中身（docx 相当・xlsx 相当）
const DOCX_V1: &[u8] = b"PK\x03\x04docx-v1";
const DOCX_V2: &[u8] = b"PK\x03\x04docx-v2";
const XLSX_V1: &[u8] = b"PK\x03\x04xlsx-v1";

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

/// 保存して、保存された commit の完全な OID を返す
fn save(repo: &Path, memo: &str) -> Result<String, Box<dyn std::error::Error>> {
    match ops().save(repo, memo)? {
        SaveOutcome::Saved { .. } => get_head_commit(repo).ok_or_else(|| "no head".into()),
        _ => Err("nothing saved".into()),
    }
}

/// 作業フォルダとリポジトリの状態（status・refs・インデックスの ls-files）を並べたもの
fn repo_fingerprint(repo: &Path) -> String {
    let (_, status, _) = run_git(repo, &["status", "--porcelain=v2", "-z"]);
    let (_, refs, _) = run_git(repo, &["for-each-ref"]);
    let (_, index, _) = run_git(repo, &["ls-files", "-s"]);
    format!("{status}\n{refs}\n{index}")
}

fn is_invalid_input(r: &Result<OfficeFile, OpsError>) -> bool {
    matches!(r, Err(OpsError::InvalidInput(_)))
}

fn is_not_found(r: &Result<OfficeFile, OpsError>) -> bool {
    matches!(r, Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound)
}

/// 版を 2 つ作る（v1 を保存し、v2 に変えて保存）。(v1 の commit, v2 の commit) を返す
fn two_versions(repo: &Path) -> Result<(String, String), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(repo.join("資料"))?;
    std::fs::write(repo.join("資料/第 3 章.docx"), DOCX_V1)?;
    write_test_file(repo, "readme.txt", "text\n")?;
    let first = save(repo, "v1")?;
    std::fs::write(repo.join("資料/第 3 章.docx"), DOCX_V2)?;
    let second = save(repo, "v2")?;
    Ok((first, second))
}

fn bytes_of(file: OfficeFile) -> Option<Vec<u8>> {
    match file {
        OfficeFile::Bytes(b) => Some(b),
        OfficeFile::TooLarge => None,
    }
}

#[test]
fn reads_the_exact_bytes_at_a_commit_and_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, second) = two_versions(&repo)?;

    let old = bytes_of(ops().read_office(&repo, &first, "資料/第 3 章.docx", 1024)?);
    assert_eq!(old.as_deref(), Some(DOCX_V1));
    let new = bytes_of(ops().read_office(&repo, &second, "資料/第 3 章.docx", 1024)?);
    assert_eq!(new.as_deref(), Some(DOCX_V2));

    // work は作業フォルダの現在の内容を読む。書き換えれば、その内容になる
    let work = bytes_of(ops().read_office(&repo, "work", "資料/第 3 章.docx", 1024)?);
    assert_eq!(work.as_deref(), Some(DOCX_V2));
    std::fs::write(repo.join("資料/第 3 章.docx"), DOCX_V1)?;
    let work = bytes_of(ops().read_office(&repo, "work", "資料/第 3 章.docx", 1024)?);
    assert_eq!(work.as_deref(), Some(DOCX_V1));
    Ok(())
}

#[test]
fn only_docx_and_xlsx_are_allowed_in_any_letter_case() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.docx", "x")?;
    write_test_file(&repo, "b.XLSX", "x")?;
    write_test_file(&repo, "c.Docx", "x")?;
    for name in ["a.docx", "b.XLSX", "c.Docx"] {
        assert!(
            ops().read_office(&repo, "work", name, 1024).is_ok(),
            "{name}"
        );
    }
    // 旧形式・マクロ付き・画像・テキストは拒否する（中身が zip でも拡張子で判定する）
    write_test_file(&repo, "old.doc", "x")?;
    write_test_file(&repo, "macro.docm", "x")?;
    write_test_file(&repo, "old.xls", "x")?;
    write_test_file(&repo, "pic.png", "x")?;
    write_test_file(&repo, "text.txt", "x")?;
    for name in [
        "old.doc",
        "macro.docm",
        "old.xls",
        "pic.png",
        "text.txt",
        "noext",
        "dir.docx/x",
    ] {
        let r = ops().read_office(&repo, "work", name, 1024);
        assert!(is_invalid_input(&r), "{name}");
    }
    Ok(())
}

#[test]
fn rev_must_be_a_full_oid_or_work() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    let short = &first[..7];

    for rev in [
        "main",
        "HEAD",
        "HEAD~1",
        "-p",
        "",
        short,
        "WORK",
        "refs/heads/main",
    ] {
        let r = ops().read_office(&repo, rev, "資料/第 3 章.docx", 1024);
        assert!(is_invalid_input(&r), "{rev:?}");
    }
    Ok(())
}

#[test]
fn unsafe_paths_are_refused_for_both_revs() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    std::fs::write(tmp.path().join("outside.docx"), DOCX_V1)?;

    for bad in [
        "../outside.docx",
        "資料/../../outside.docx",
        "..\\outside.docx",
        "/etc/outside.docx",
        "C:\\Windows\\outside.docx",
        "\\\\server\\share\\x.docx",
        ".git/config.docx",
        ".GIT/x.docx",
        "sub/.git/x.docx",
        ":(top)readme.docx",
        "",
        "tab\tname.docx",
        "new\nline.docx",
        "nul\u{0}.docx",
    ] {
        for rev in [first.as_str(), "work"] {
            let r = ops().read_office(&repo, rev, bad, 1024);
            assert!(is_invalid_input(&r), "{bad:?} at {rev}");
        }
    }
    Ok(())
}

#[test]
fn missing_files_and_folders_are_errors() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    std::fs::create_dir_all(repo.join("folder.docx"))?;

    // 存在しない（その時点にも、作業フォルダにも無い）ファイル
    assert!(is_not_found(&ops().read_office(
        &repo,
        &first,
        "missing.docx",
        1024
    )));
    assert!(is_not_found(&ops().read_office(
        &repo,
        "work",
        "missing.docx",
        1024
    )));
    // その時点に無い（後から追加した）ファイル
    write_test_file(&repo, "later.xlsx", "later")?;
    save(&repo, "later")?;
    assert!(is_not_found(&ops().read_office(
        &repo,
        &first,
        "later.xlsx",
        1024
    )));
    // フォルダは通常のファイルではない
    assert!(is_not_found(&ops().read_office(
        &repo,
        "work",
        "folder.docx",
        1024
    )));
    Ok(())
}

#[test]
fn sizes_over_the_limit_are_reported_as_too_large_without_bytes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    let size = DOCX_V1.len() as u64;

    // ちょうどの大きさは読める。1 バイトでも小さければ TooLarge（エラーではない）
    let exact = ops().read_office(&repo, &first, "資料/第 3 章.docx", size)?;
    assert_eq!(bytes_of(exact).as_deref(), Some(DOCX_V1));
    let small = ops().read_office(&repo, &first, "資料/第 3 章.docx", size - 1)?;
    assert_eq!(small, OfficeFile::TooLarge);
    let small_work = ops().read_office(&repo, "work", "資料/第 3 章.docx", size - 1)?;
    assert_eq!(small_work, OfficeFile::TooLarge);
    Ok(())
}

#[test]
fn symlinks_out_of_the_project_are_refused_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let outside = tmp.path().join("secret.docx");
    std::fs::write(&outside, DOCX_V1)?;
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, repo.join("link.docx")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside, repo.join("link.docx")).is_ok();
    if !linked {
        // Windows で開発者モードなどが無くシンボリックリンクを作れない環境では、このテストを飛ばす
        eprintln!("skip: symlink を作成できない環境");
        return Ok(());
    }
    // リンク先が外のファイルなら拒否する（中身は読まない）
    assert!(is_invalid_input(&ops().read_office(
        &repo,
        "work",
        "link.docx",
        1024
    )));
    Ok(())
}

#[test]
fn reading_changes_neither_the_work_folder_nor_the_refs() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, second) = two_versions(&repo)?;
    // 作業フォルダに未保存の変更と、新規（未追跡）のファイルを置く
    std::fs::write(repo.join("資料/第 3 章.docx"), DOCX_V1)?;
    std::fs::write(repo.join("untracked.xlsx"), XLSX_V1)?;

    let before = repo_fingerprint(&repo);
    let before_file = std::fs::read(repo.join("資料/第 3 章.docx"))?;

    ops().read_office(&repo, &first, "資料/第 3 章.docx", 1024)?;
    ops().read_office(&repo, &second, "資料/第 3 章.docx", 1024)?;
    ops().read_office(&repo, "work", "資料/第 3 章.docx", 1024)?;
    ops().read_office(&repo, "work", "untracked.xlsx", 1024)?;
    // 拒否された読み取りも、何も変えない
    let _ = ops().read_office(&repo, "main", "資料/第 3 章.docx", 1024);
    let _ = ops().read_office(&repo, "work", "../x.docx", 1024);
    let _ = ops().read_office(&repo, "work", "old.doc", 1024);

    assert_eq!(repo_fingerprint(&repo), before);
    assert_eq!(std::fs::read(repo.join("資料/第 3 章.docx"))?, before_file);
    assert!(repo.join("untracked.xlsx").exists());
    Ok(())
}
