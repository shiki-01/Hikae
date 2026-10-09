// 画像の読み取り（Ops::read_blob、設計書 9.3 app-blob）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 読み取りのみであることを、作業フォルダ・インデックス・refs の変化で確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Identity, Ops, OpsError, SaveOutcome};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 1x1 相当の PNG の先頭（署名と IHDR）。中身の正しさは読み取りでは検査しないため、版の違いが分かればよい
const PNG_V1: &[u8] =
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00v1";
const PNG_V2: &[u8] =
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00v2";

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

fn is_invalid_input(r: &Result<core_ops::Blob, OpsError>) -> bool {
    matches!(r, Err(OpsError::InvalidInput(_)))
}

/// 版を 2 つ作る（v1 を保存し、v2 に変えて保存）。(v1 の commit, v2 の commit) を返す
fn two_versions(repo: &Path) -> Result<(String, String), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(repo.join("資料"))?;
    std::fs::write(repo.join("資料/図 1.PNG"), PNG_V1)?;
    write_test_file(repo, "readme.txt", "text\n")?;
    let first = save(repo, "v1")?;
    std::fs::write(repo.join("資料/図 1.PNG"), PNG_V2)?;
    let second = save(repo, "v2")?;
    Ok((first, second))
}

#[test]
fn reads_the_exact_bytes_at_a_commit_and_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, second) = two_versions(&repo)?;

    // 大文字の拡張子でも、mime は画像のもの。版ごとに内容が違う
    let old = ops().read_blob(&repo, &first, "資料/図 1.PNG", 1024)?;
    assert_eq!(old.bytes, PNG_V1);
    assert_eq!(old.mime, "image/png");
    let new = ops().read_blob(&repo, &second, "資料/図 1.PNG", 1024)?;
    assert_eq!(new.bytes, PNG_V2);
    assert_ne!(old.bytes, new.bytes);

    // work は作業フォルダの現在の内容を読む。作業フォルダを書き換えれば、その内容になる
    let work = ops().read_blob(&repo, "work", "資料/図 1.PNG", 1024)?;
    assert_eq!(work.bytes, PNG_V2);
    std::fs::write(repo.join("資料/図 1.PNG"), PNG_V1)?;
    assert_eq!(
        ops().read_blob(&repo, "work", "資料/図 1.PNG", 1024)?.bytes,
        PNG_V1
    );
    Ok(())
}

#[test]
fn every_allowed_extension_maps_to_its_mime_and_svg_is_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let cases = [
        ("a.png", "image/png"),
        ("b.jpg", "image/jpeg"),
        ("c.JPEG", "image/jpeg"),
        ("d.gif", "image/gif"),
        ("e.WebP", "image/webp"),
        ("f.bmp", "image/bmp"),
    ];
    for (name, mime) in cases {
        write_test_file(&repo, name, "not really decoded")?;
        assert_eq!(
            ops().read_blob(&repo, "work", name, 1024)?.mime,
            mime,
            "{name}"
        );
    }
    // svg・テキスト・拡張子なしは拒否（中身が画像でも拡張子で判定する）
    for name in ["icon.svg", "ICON.SVG", "notes.txt", "noext", "dir.png/x"] {
        write_test_file(&repo, name, "<svg/>")?;
        let r = ops().read_blob(&repo, "work", name, 1024);
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

    // 完全な OID は通る（大文字の 16 進も受け付ける）
    assert!(ops()
        .read_blob(&repo, &first, "資料/図 1.PNG", 1024)
        .is_ok());
    assert!(ops()
        .read_blob(&repo, &first.to_uppercase(), "資料/図 1.PNG", 1024)
        .is_ok());

    // 完全な OID・work 以外は拒否する（ブランチ名・HEAD・短縮形・オプションに見える値・空）
    for rev in [
        "main",
        "HEAD",
        "HEAD~1",
        "-p",
        "--",
        "",
        short,
        "work ",
        "WORK",
        "refs/heads/main",
    ] {
        let r = ops().read_blob(&repo, rev, "資料/図 1.PNG", 1024);
        assert!(is_invalid_input(&r), "{rev:?}");
    }
    // 40 桁でも 16 進でなければ拒否する
    let not_hex = "g".repeat(40);
    assert!(is_invalid_input(&ops().read_blob(
        &repo,
        &not_hex,
        "資料/図 1.PNG",
        1024
    )));
    Ok(())
}

#[test]
fn unsafe_paths_are_refused_for_both_revs() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    // 外に画像を置いても読めないこと
    let outside = tmp.path().join("outside.png");
    std::fs::write(&outside, PNG_V1)?;

    for bad in [
        "../outside.png",
        "資料/../../outside.png",
        "..\\outside.png",
        "/etc/outside.png",
        "C:\\Windows\\outside.png",
        "\\\\server\\share\\x.png",
        ".git/config.png",
        ".GIT/x.png",
        "sub/.git/x.png",
        ":(top)readme.png",
        "",
        "tab\tname.png",
        "new\nline.png",
        "nul\u{0}.png",
    ] {
        for rev in [first.as_str(), "work"] {
            let r = ops().read_blob(&repo, rev, bad, 1024);
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
    std::fs::create_dir_all(repo.join("folder.png"))?;

    // 存在しない（その時点にも、作業フォルダにも無い）ファイル
    assert!(ops().read_blob(&repo, &first, "missing.png", 1024).is_err());
    assert!(ops().read_blob(&repo, "work", "missing.png", 1024).is_err());
    // その時点に無い（後から追加した）ファイル
    write_test_file(&repo, "later.png", "later")?;
    save(&repo, "later")?;
    assert!(ops().read_blob(&repo, &first, "later.png", 1024).is_err());
    // フォルダは通常のファイルではない
    assert!(ops().read_blob(&repo, "work", "folder.png", 1024).is_err());
    Ok(())
}

#[test]
fn sizes_over_the_limit_are_refused_before_reading() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    let size = PNG_V1.len() as u64;

    // ちょうどの大きさは読める。1 バイトでも小さければ拒否する
    assert_eq!(
        ops().read_blob(&repo, &first, "資料/図 1.PNG", size)?.bytes,
        PNG_V1
    );
    assert!(is_invalid_input(&ops().read_blob(
        &repo,
        &first,
        "資料/図 1.PNG",
        size - 1
    )));
    assert!(is_invalid_input(&ops().read_blob(
        &repo,
        "work",
        "資料/図 1.PNG",
        size - 1
    )));
    Ok(())
}

#[test]
fn symlinks_out_of_the_project_are_refused_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let outside = tmp.path().join("secret.png");
    std::fs::write(&outside, PNG_V1)?;
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, repo.join("link.png")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside, repo.join("link.png")).is_ok();
    if !linked {
        // Windows で開発者モードなどが無くシンボリックリンクを作れない環境では、このテストを飛ばす
        eprintln!("skip: symlink を作成できない環境");
        return Ok(());
    }
    // リンク先が外のファイルなら拒否する（中身は読まない）
    assert!(is_invalid_input(
        &ops().read_blob(&repo, "work", "link.png", 1024)
    ));
    Ok(())
}

#[test]
fn reading_changes_neither_the_work_folder_nor_the_refs() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, second) = two_versions(&repo)?;
    // 作業フォルダに未保存の変更と、新規（未追跡）の画像を置く
    std::fs::write(repo.join("資料/図 1.PNG"), PNG_V1)?;
    std::fs::write(repo.join("untracked.png"), PNG_V2)?;

    let before = repo_fingerprint(&repo);
    let before_files = std::fs::read(repo.join("資料/図 1.PNG"))?;

    ops().read_blob(&repo, &first, "資料/図 1.PNG", 1024)?;
    ops().read_blob(&repo, &second, "資料/図 1.PNG", 1024)?;
    ops().read_blob(&repo, "work", "資料/図 1.PNG", 1024)?;
    ops().read_blob(&repo, "work", "untracked.png", 1024)?;
    // 拒否された読み取りも、何も変えない
    let _ = ops().read_blob(&repo, "main", "資料/図 1.PNG", 1024);
    let _ = ops().read_blob(&repo, "work", "../x.png", 1024);

    assert_eq!(repo_fingerprint(&repo), before);
    assert_eq!(std::fs::read(repo.join("資料/図 1.PNG"))?, before_files);
    assert!(repo.join("untracked.png").exists());
    Ok(())
}

#[test]
fn full_commit_id_resolves_a_short_id_and_refuses_unknown_ones() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let (first, _) = two_versions(&repo)?;
    assert_eq!(ops().full_commit_id(&repo, &first[..7])?, first);
    assert!(ops().full_commit_id(&repo, "no-such-commit").is_err());
    assert!(ops().full_commit_id(&repo, "-p").is_err());
    Ok(())
}
