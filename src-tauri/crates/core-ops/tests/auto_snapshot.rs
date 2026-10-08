// ファイル監視による自動保存（設計書 6.2、7章）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。閾値はテスト用に小さくして、巨大なファイルを作らない。
// 不変条件 4（作業フォルダとインデックスを変更しない）、5（refs/hikae/ の外を触らない）を確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{AutoSnapshotOutcome, AutoSnapshotSkip, Identity, Ops, SaveOutcome, SizeLimits};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LIMITS: SizeLimits = SizeLimits {
    warn_bytes: 1_000,
    block_bytes: 5_000,
};

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(&repo, None, &identity("tester"))?;
    Ok(repo)
}

fn write_bytes(repo: &Path, rel: &str, len: usize) -> TestResult {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, vec![b'x'; len])?;
    Ok(())
}

fn snapshot_refs(repo: &Path) -> Vec<String> {
    let (_, out, _) = run_git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hikae/snapshots/",
        ],
    );
    out.lines().map(str::to_string).collect()
}

fn files_in(repo: &Path, rev: &str) -> Vec<String> {
    let (_, out, _) = run_git(
        repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            rev,
        ],
    );
    let mut v: Vec<String> = out.lines().map(str::to_string).collect();
    v.sort();
    v
}

fn all_refs(repo: &Path) -> String {
    run_git(repo, &["for-each-ref"]).1
}

/// 作業フォルダ（.git 以外）の内容とインデックスのファイルを控える
fn fingerprint(repo: &Path) -> (Vec<(String, Vec<u8>)>, Vec<u8>) {
    let mut files = Vec::new();
    let mut stack = vec![repo.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(repo)
                    .expect("prefix")
                    .to_string_lossy()
                    .to_string();
                files.push((rel, std::fs::read(&path).expect("read")));
            }
        }
    }
    files.sort();
    let index = std::fs::read(repo.join(".git").join("index")).unwrap_or_default();
    (files, index)
}

fn created(outcome: AutoSnapshotOutcome) -> (String, Vec<String>) {
    match outcome {
        AutoSnapshotOutcome::Created {
            snapshot_ref,
            excluded_large,
        } => (
            snapshot_ref,
            excluded_large.into_iter().map(|f| f.path).collect(),
        ),
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn creates_a_snapshot_without_touching_the_working_folder_or_index() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    ops().save(&repo, "first")?;
    let base = snapshot_refs(&repo).len();

    // 変更済み・ステージ済み・未追跡が混ざった状態
    write_bytes(&repo, "a.txt", 20)?;
    write_bytes(&repo, "staged.txt", 5)?;
    run_git(&repo, &["add", "staged.txt"]);
    write_bytes(&repo, "新しい 文書.txt", 7)?;

    let before = fingerprint(&repo);
    let head_before = get_head_commit(&repo);
    let status_before = run_git(&repo, &["status", "--porcelain=v2", "-z"]).1;

    let (snapshot_ref, excluded) = created(ops().auto_snapshot(&repo, LIMITS)?);
    assert!(excluded.is_empty());
    assert!(snapshot_ref.starts_with("refs/hikae/snapshots/main/"));

    // 作業フォルダ・インデックス・HEAD・status は何も変わらない
    assert_eq!(fingerprint(&repo), before);
    assert_eq!(get_head_commit(&repo), head_before);
    assert_eq!(
        run_git(&repo, &["status", "--porcelain=v2", "-z"]).1,
        status_before
    );
    assert!(!repo.join(".git").join("index.lock").exists());

    // スナップショットには未保存の変更がすべて入る
    assert_eq!(
        files_in(&repo, &snapshot_ref),
        vec!["a.txt", "staged.txt", "新しい 文書.txt"]
    );
    // 増えた ref は refs/hikae/snapshots/ の中の 1 つだけ（ブランチやタグは増えない）
    let refs = all_refs(&repo);
    assert!(refs.contains("refs/heads/main"), "{refs}");
    assert_eq!(
        refs.lines()
            .filter(|l| !l.contains("refs/heads/main"))
            .count(),
        base + 1,
        "{refs}"
    );
    assert!(refs
        .lines()
        .all(|l| l.contains("refs/heads/main") || l.contains("refs/hikae/snapshots/main/")));
    Ok(())
}

#[test]
fn does_nothing_when_clean_or_when_content_matches_the_previous_snapshot() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    ops().save(&repo, "first")?;
    let base = snapshot_refs(&repo).len();

    // 変更なし
    assert_eq!(
        ops().auto_snapshot(&repo, LIMITS)?,
        AutoSnapshotOutcome::Unchanged
    );
    assert_eq!(snapshot_refs(&repo).len(), base);

    // 変更 → 1 回目は作る、同じ内容のまま 2 回目は作らない
    write_bytes(&repo, "a.txt", 30)?;
    created(ops().auto_snapshot(&repo, LIMITS)?);
    assert_eq!(
        ops().auto_snapshot(&repo, LIMITS)?,
        AutoSnapshotOutcome::Unchanged
    );
    assert_eq!(snapshot_refs(&repo).len(), base + 1);

    // さらに変更すれば新しく作る
    write_bytes(&repo, "a.txt", 31)?;
    created(ops().auto_snapshot(&repo, LIMITS)?);
    assert_eq!(snapshot_refs(&repo).len(), base + 2);
    Ok(())
}

#[test]
fn large_files_are_left_out_and_reported() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    write_bytes(&repo, "tracked.bin", 100)?;
    ops().save(&repo, "first")?;
    let base = snapshot_refs(&repo).len();

    write_bytes(&repo, "a.txt", 11)?;
    // 追跡済みの大きいファイルの変更（警告）、未追跡の巨大ファイル（保存不可）、未追跡の中くらい（警告）
    write_bytes(&repo, "tracked.bin", 2_000)?;
    write_bytes(&repo, "sub/huge.mp4", 9_000)?;
    write_bytes(&repo, "mid.psd", 1_500)?;

    let (snapshot_ref, excluded) = created(ops().auto_snapshot(&repo, LIMITS)?);
    assert_eq!(excluded, vec!["mid.psd", "sub/huge.mp4", "tracked.bin"]);

    // 含めたのは通常のファイルだけ。追跡済みの大きいファイルは HEAD の版のまま
    assert_eq!(files_in(&repo, &snapshot_ref), vec!["a.txt", "tracked.bin"]);
    let (_, size, _) = run_git(
        &repo,
        &["cat-file", "-s", &format!("{snapshot_ref}:tracked.bin")],
    );
    assert_eq!(size.trim(), "100");
    // 作業フォルダの大きいファイルは消えない
    assert!(repo.join("sub/huge.mp4").exists());
    // 作った自動保存は 1 つだけ
    assert_eq!(snapshot_refs(&repo).len(), base + 1);
    Ok(())
}

#[test]
fn only_large_files_changed_makes_no_snapshot() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    ops().save(&repo, "first")?;
    let base = snapshot_refs(&repo).len();

    write_bytes(&repo, "huge.bin", 9_000)?;
    assert_eq!(
        ops().auto_snapshot(&repo, LIMITS)?,
        AutoSnapshotOutcome::Unchanged
    );
    assert_eq!(snapshot_refs(&repo).len(), base);
    Ok(())
}

#[test]
fn works_before_the_first_save_and_records_the_signing_user() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "draft.txt", 10)?;

    let ops = Ops::new(GitRunner::from_path_env()).with_signing_user(Some((42, "octocat")));
    let (snapshot_ref, _) = created(ops.auto_snapshot(&repo, LIMITS)?);
    assert_eq!(files_in(&repo, &snapshot_ref), vec!["draft.txt"]);
    let (_, author, _) = run_git(&repo, &["log", "-1", "--format=%an <%ae>", &snapshot_ref]);
    assert_eq!(
        author.trim(),
        "octocat <42+octocat@users.noreply.github.com>"
    );
    // まだ保存が無い（HEAD が無い）ままで、作業フォルダも変わらない
    assert_eq!(get_head_commit(&repo), None);
    Ok(())
}

#[test]
fn does_not_snapshot_while_a_merge_conflict_is_unresolved() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    ops().save(&repo, "base")?;
    let base = snapshot_refs(&repo).len();

    run_git(&repo, &["checkout", "-b", "other"]);
    std::fs::write(repo.join("a.txt"), "other side\n")?;
    run_git(&repo, &["commit", "-am", "other"]);
    run_git(&repo, &["checkout", "main"]);
    std::fs::write(repo.join("a.txt"), "main side\n")?;
    run_git(&repo, &["commit", "-am", "main"]);
    let (code, _, _) = run_git(&repo, &["merge", "other"]);
    assert_ne!(code, 0, "競合するはず");

    assert_eq!(
        ops().auto_snapshot(&repo, LIMITS)?,
        AutoSnapshotOutcome::Skipped(AutoSnapshotSkip::Conflict)
    );
    assert_eq!(snapshot_refs(&repo).len(), base);
    Ok(())
}

#[test]
fn a_manual_save_afterwards_still_works_and_nothing_is_duplicated() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "a.txt", 10)?;
    ops().save(&repo, "first")?;

    write_bytes(&repo, "a.txt", 40)?;
    created(ops().auto_snapshot(&repo, LIMITS)?);
    // 自動保存のあとも、通常の保存はそのまま通る（インデックスが壊れていない）
    let outcome = ops().save(&repo, "second")?;
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    // 保存後は変更が無いので、自動保存は作らない
    assert_eq!(
        ops().auto_snapshot(&repo, LIMITS)?,
        AutoSnapshotOutcome::Unchanged
    );
    Ok(())
}
