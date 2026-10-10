// プレビュー用の読み取り（pdf・pptx・テキスト）と、画面の時点 ID の解決（this・cloud を含む）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。競合は PC-A と PC-B の実際の保存・取り込みで作る。
// どの操作でも、作業フォルダ・インデックス・refs が変わらないことを確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    FileDiffOutcome, Identity, OfficeFile, Ops, OpsError, PullOutcome, SaveOutcome, TextFile,
    WORK_REV,
};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// テキストとして読める上限（設計書 5 章の 1 MiB。プレビュー用）
const ONE_MIB: u64 = 1024 * 1024;

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn ops_named(pc: &str) -> Ops {
    ops().with_pc_name(Some(pc.to_string()))
}

/// 作業フォルダ・インデックス・refs の状態。読み取りの前後で同じであることを確かめる
fn repo_fingerprint(repo: &Path) -> String {
    let (_, status, _) = run_git(repo, &["status", "--porcelain=v2", "-z"]);
    let (_, refs, _) = run_git(repo, &["for-each-ref"]);
    let (_, index, _) = run_git(repo, &["ls-files", "-s"]);
    format!("{status}\n{refs}\n{index}")
}

/// 保存して、保存された commit の完全な OID を返す
fn save(repo: &Path, memo: &str) -> Result<String, Box<dyn std::error::Error>> {
    match ops().save(repo, memo)? {
        SaveOutcome::Saved { .. } => get_head_commit(repo).ok_or_else(|| "no head".into()),
        _ => Err("nothing saved".into()),
    }
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(&repo, None, &identity("tester"))?;
    Ok(repo)
}

/// PC-B で取り込みの競合が残っている状態。`conflict.txt` は両方変更（PC-A は "A1"、PC-B は "B1"）。
/// 戻り値は (一時ディレクトリ, PC-B のフォルダ)。PC-B の HEAD は PC-B の版、MERGE_HEAD はクラウドの版になる
struct Conflicted {
    _tmp: tempfile::TempDir,
    pc_b: PathBuf,
}

fn conflicted() -> Result<Conflicted, Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    let a = ops_named("PC-A");
    a.init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    std::fs::write(pc_a.join("conflict.txt"), "line1\nline2\nline3\n")?;
    a.save(&pc_a, "initial")?;
    a.upload(&pc_a)?;
    ops_named("PC-B").clone_project(&url, &pc_b, &identity("pc-b"))?;

    std::fs::write(pc_a.join("conflict.txt"), "A1\nline2\nline3\n")?;
    a.save(&pc_a, "A changes")?;
    a.upload(&pc_a)?;

    let b = ops_named("PC-B");
    std::fs::write(pc_b.join("conflict.txt"), "B1\nline2\nline3\n")?;
    b.save(&pc_b, "B changes")?;
    match b.pull(&pc_b)? {
        PullOutcome::Conflicted { .. } => {}
        _ => return Err("expected Conflicted".into()),
    }
    Ok(Conflicted { _tmp: tmp, pc_b })
}

// ---------------------------------------------------------------- pdf と pptx の読み取り

#[test]
fn reads_pdf_as_a_blob_with_the_pdf_mime_at_a_commit_and_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let pdf = b"%PDF-1.4\n%fake\n".to_vec();
    std::fs::write(repo.join("manual.PDF"), &pdf)?;
    let commit = save(&repo, "pdf")?;

    let at_commit = ops().read_blob(&repo, &commit, "manual.PDF", ONE_MIB)?;
    assert_eq!(at_commit.bytes, pdf);
    assert_eq!(at_commit.mime, "application/pdf");

    let in_work = ops().read_blob(&repo, WORK_REV, "manual.PDF", ONE_MIB)?;
    assert_eq!(in_work.bytes, pdf);
    assert_eq!(in_work.mime, "application/pdf");
    Ok(())
}

#[test]
fn reads_pptx_as_office_bytes_and_refuses_legacy_office_and_other_types() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let pptx = b"PK\x03\x04deck".to_vec();
    std::fs::write(repo.join("deck.pptx"), &pptx)?;
    std::fs::write(repo.join("old.ppt"), b"legacy")?;
    std::fs::write(repo.join("notes.txt"), "text\n")?;
    let commit = save(&repo, "decks")?;

    match ops().read_office(&repo, &commit, "deck.pptx", ONE_MIB)? {
        OfficeFile::Bytes(bytes) => assert_eq!(bytes, pptx),
        OfficeFile::TooLarge => return Err("pptx should be read".into()),
    }
    assert!(matches!(
        ops().read_office(&repo, &commit, "old.ppt", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().read_blob(&repo, &commit, "notes.txt", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));
    Ok(())
}

// ---------------------------------------------------------------- read_text

#[test]
fn read_text_returns_utf8_text_of_any_extension_at_a_commit_and_in_the_work_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(repo.join("memo.md"), "# 見出し\n本文\n")?;
    std::fs::write(repo.join("data.custom"), "x,y\n1,2\n")?;
    let commit = save(&repo, "text")?;

    let expected = Some("# 見出し\n本文\n".to_string());
    assert_eq!(
        ops().read_text(&repo, &commit, "memo.md", ONE_MIB),
        expected
    );
    assert_eq!(
        ops().read_text(&repo, WORK_REV, "memo.md", ONE_MIB),
        expected
    );
    assert_eq!(
        ops().read_text(&repo, &commit, "data.custom", ONE_MIB),
        Some("x,y\n1,2\n".to_string())
    );
    Ok(())
}

#[test]
fn read_text_file_classifies_binary_nul_invalid_utf8_and_too_large() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    std::fs::write(repo.join("bin.dat"), [0u8, b'a', b'b'])?;
    std::fs::write(repo.join("latin.txt"), [0xffu8, b'a', b'\n'])?;
    std::fs::write(repo.join("big.txt"), vec![b'a'; (ONE_MIB + 1) as usize])?;
    std::fs::write(repo.join("exact.txt"), vec![b'b'; ONE_MIB as usize])?;
    let commit = save(&repo, "kinds")?;

    assert_eq!(
        ops().read_text_file(&repo, &commit, "bin.dat", ONE_MIB)?,
        TextFile::Binary
    );
    assert_eq!(
        ops().read_text_file(&repo, WORK_REV, "latin.txt", ONE_MIB)?,
        TextFile::Binary
    );
    assert_eq!(
        ops().read_text_file(&repo, &commit, "big.txt", ONE_MIB)?,
        TextFile::TooLarge
    );
    // 上限ちょうどは読める
    match ops().read_text_file(&repo, WORK_REV, "exact.txt", ONE_MIB)? {
        TextFile::Text(text) => assert_eq!(text.len() as u64, ONE_MIB),
        other => return Err(format!("expected text, got {other:?}").into()),
    }
    // 簡易版は、読めないものを None にする
    assert_eq!(ops().read_text(&repo, &commit, "big.txt", ONE_MIB), None);
    assert_eq!(ops().read_text(&repo, &commit, "bin.dat", ONE_MIB), None);
    Ok(())
}

#[test]
fn read_text_of_a_missing_file_is_not_found_and_none() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "a\n")?;
    let commit = save(&repo, "a")?;

    assert!(matches!(
        ops().read_text_file(&repo, &commit, "missing.txt", ONE_MIB),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
    ));
    assert!(matches!(
        ops().read_text_file(&repo, WORK_REV, "missing.txt", ONE_MIB),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
    ));
    assert_eq!(
        ops().read_text(&repo, &commit, "missing.txt", ONE_MIB),
        None
    );
    Ok(())
}

#[test]
fn read_text_refuses_bad_paths_and_leaves_the_repository_unchanged() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "a\n")?;
    let commit = save(&repo, "a")?;
    // 作業フォルダの変更（未保存）も、読み取りで変わらないこと
    std::fs::write(repo.join("a.txt"), "changed\n")?;
    let before = repo_fingerprint(&repo);

    assert!(matches!(
        ops().read_text_file(&repo, &commit, "../outside.txt", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().read_text_file(&repo, &commit, ".git/config", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().read_text_file(&repo, "not-a-rev", "a.txt", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));
    assert_eq!(
        ops().read_text(&repo, WORK_REV, "a.txt", ONE_MIB),
        Some("changed\n".to_string())
    );
    assert_eq!(repo_fingerprint(&repo), before);
    Ok(())
}

// ---------------------------------------------------------------- 時点 ID の解決

#[test]
fn resolves_work_short_ids_and_refuses_unknown_words() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "a\n")?;
    let commit = save(&repo, "a")?;
    let before = repo_fingerprint(&repo);

    assert_eq!(ops().resolve_point(&repo, "current")?, WORK_REV);
    assert_eq!(ops().resolve_point(&repo, WORK_REV)?, WORK_REV);
    assert_eq!(ops().resolve_point(&repo, &commit[..8])?, commit);
    assert_eq!(ops().resolve_point(&repo, &commit)?, commit);
    // 競合の途中でないので、cloud は版が無い（NotFound）
    assert!(matches!(
        ops().resolve_point(&repo, "cloud"),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
    ));
    // 形の不正な値は InvalidInput。存在しない短縮 ID は NotFound
    assert!(matches!(
        ops().resolve_point(&repo, "main"),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().resolve_point(&repo, "HEAD"),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().resolve_point(&repo, "0000abcd"),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
    ));
    assert_eq!(repo_fingerprint(&repo), before);
    Ok(())
}

#[test]
fn this_and_cloud_resolve_to_head_and_merge_head_during_a_conflict() -> TestResult {
    let c = conflicted()?;
    let repo = c.pc_b.as_path();
    let before = repo_fingerprint(repo);

    let this_oid = ops().resolve_point(repo, "this")?;
    let cloud_oid = ops().resolve_point(repo, "cloud")?;
    assert_eq!(Some(this_oid.clone()), get_head_commit(repo));
    assert_eq!(
        run_git(repo, &["rev-parse", "MERGE_HEAD"]).1.trim(),
        cloud_oid
    );
    assert_ne!(this_oid, cloud_oid);

    // 両方の版で、競合ファイルの内容がそれぞれの側の内容になる（読み取りだけで確かめる）
    let this_text = ops().read_text(repo, &this_oid, "conflict.txt", ONE_MIB);
    let cloud_text = ops().read_text(repo, &cloud_oid, "conflict.txt", ONE_MIB);
    assert_eq!(this_text, Some("B1\nline2\nline3\n".to_string()));
    assert_eq!(cloud_text, Some("A1\nline2\nline3\n".to_string()));

    // 画像・Office の読み取りも、解決した OID で動く（pdf と pptx は拡張子だけ確かめる）
    assert!(ops()
        .read_blob(repo, &cloud_oid, "conflict.txt", ONE_MIB)
        .is_err());
    assert!(matches!(
        ops().read_office(repo, &this_oid, "conflict.txt", ONE_MIB),
        Err(OpsError::InvalidInput(_))
    ));

    // 2 つの版のテキスト差分（画面の diff が使う経路）が、片側だけの変更を返す
    match ops().diff_file(repo, &this_oid, &cloud_oid, Some("conflict.txt"))? {
        FileDiffOutcome::Lines(lines) => {
            assert!(lines.iter().any(|l| l.content == "B1"));
            assert!(lines.iter().any(|l| l.content == "A1"));
        }
        other => return Err(format!("expected lines, got {other:?}").into()),
    }
    assert_eq!(repo_fingerprint(repo), before);
    Ok(())
}

#[test]
fn a_point_resolved_to_work_during_a_conflict_still_reads_the_working_file() -> TestResult {
    let c = conflicted()?;
    let repo = c.pc_b.as_path();
    let this_oid = ops().resolve_point(repo, "this")?;
    let work = ops().resolve_point(repo, "current")?;
    assert_eq!(work, WORK_REV);
    // 作業フォルダのファイルは競合マーカー付きの内容
    let text = ops()
        .read_text(repo, &work, "conflict.txt", ONE_MIB)
        .unwrap_or_default();
    assert!(text.contains("<<<<<<<"));
    // this との差分は作業フォルダを右側にして取れる
    assert!(matches!(
        ops().diff_file(repo, &this_oid, &work, Some("conflict.txt"))?,
        FileDiffOutcome::Lines(_)
    ));
    Ok(())
}
