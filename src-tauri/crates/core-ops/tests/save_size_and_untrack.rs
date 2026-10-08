// 保存前のサイズ検査（設計書 4.1 / 5章 E07・E08）と、取り込み手順 8（4.3: 相手が追跡を外した
// ファイルの書き戻し）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 閾値はテスト用に小さくして、巨大なファイルを作らない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    Identity, LargeFile, Ops, OpsError, PullOutcome, SaveOptions, SaveOutcome, SizeLimits,
};
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

fn options() -> SaveOptions {
    SaveOptions {
        limits: LIMITS,
        ..SaveOptions::default()
    }
}

fn write_bytes(repo: &Path, rel: &str, len: usize) -> TestResult {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, vec![b'x'; len])?;
    Ok(())
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(&repo, None, &identity("tester"))?;
    Ok(repo)
}

fn tracked(repo: &Path) -> Vec<String> {
    let (_, out, _) = run_git(repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    out.lines().map(str::to_string).collect()
}

fn hikae_refs(repo: &Path) -> String {
    run_git(repo, &["for-each-ref", "refs/hikae/"]).1
}

// ---------------------------------------------------------------- サイズ検査

#[test]
fn oversized_files_stop_the_save_without_changing_anything() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "small.txt", 10)?;
    write_bytes(&repo, "mid.psd", 2_000)?;
    write_bytes(&repo, "動画 1.mp4", 6_000)?;
    write_bytes(&repo, "sub/deep/huge.bin", 9_000)?;

    let outcome = ops().save_with(&repo, "memo", &options())?;
    let SaveOutcome::NeedsSizeDecision(found) = outcome else {
        panic!("expected NeedsSizeDecision, got {outcome:?}");
    };
    assert_eq!(
        found.blocked,
        vec![
            LargeFile {
                path: "sub/deep/huge.bin".to_string(),
                size: 9_000
            },
            LargeFile {
                path: "動画 1.mp4".to_string(),
                size: 6_000
            },
        ]
    );
    assert_eq!(
        found.warned,
        vec![LargeFile {
            path: "mid.psd".to_string(),
            size: 2_000
        }]
    );

    // 何も変更していない: 保存もインデックスへの追加も復元点も無く、ファイルはそのまま
    assert_eq!(get_head_commit(&repo), None);
    assert!(run_git(&repo, &["ls-files"]).1.trim().is_empty());
    assert!(hikae_refs(&repo).trim().is_empty());
    assert!(repo.join("動画 1.mp4").exists() && repo.join("small.txt").exists());
    assert!(!repo.join(".gitignore").exists());

    // 読み取り専用の検査も同じ結果を返す
    assert_eq!(ops().check_save_sizes(&repo, LIMITS)?, found);
    Ok(())
}

#[test]
fn accepting_warnings_does_not_accept_blocked_files() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "mid.psd", 2_000)?;
    write_bytes(&repo, "big.mp4", 6_000)?;

    let mut opts = options();
    opts.accept_warned = true;
    let SaveOutcome::NeedsSizeDecision(found) = ops().save_with(&repo, "memo", &opts)? else {
        panic!("blocked file must still need a decision");
    };
    assert_eq!(found.blocked.len(), 1);
    assert!(found.warned.is_empty(), "承諾済みの警告は再度出さない");
    assert_eq!(get_head_commit(&repo), None);
    Ok(())
}

#[test]
fn warnings_alone_are_saved_once_accepted() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "mid.psd", 2_000)?;

    assert!(matches!(
        ops().save_with(&repo, "memo", &options())?,
        SaveOutcome::NeedsSizeDecision(_)
    ));

    let mut opts = options();
    opts.accept_warned = true;
    assert!(matches!(
        ops().save_with(&repo, "memo", &opts)?,
        SaveOutcome::Saved { .. }
    ));
    assert_eq!(tracked(&repo), vec!["mid.psd".to_string()]);
    Ok(())
}

#[test]
fn excluded_files_are_left_out_of_the_save_but_stay_on_disk() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "small.txt", 10)?;
    write_bytes(&repo, "mid.psd", 2_000)?;
    write_bytes(&repo, "動画 1.mp4", 6_000)?;
    std::fs::write(repo.join(".gitignore"), "*.log")?;

    let mut opts = options();
    opts.accept_warned = true;
    opts.exclude = vec!["動画 1.mp4".to_string()];
    let SaveOutcome::Saved { restore_point, .. } = ops().save_with(&repo, "memo", &opts)? else {
        panic!("expected Saved");
    };

    let mut files = tracked(&repo);
    files.sort();
    assert_eq!(
        files,
        vec![
            ".gitignore".to_string(),
            "mid.psd".to_string(),
            "small.txt".to_string()
        ]
    );
    // 外したファイルは作業フォルダに残り、追記は既存の内容を保つ
    assert_eq!(std::fs::metadata(repo.join("動画 1.mp4"))?.len(), 6_000);
    assert_eq!(
        std::fs::read_to_string(repo.join(".gitignore"))?,
        "*.log\n/動画 1.mp4\n"
    );
    // 保存後に未保存の変更として残らない
    assert!(run_git(&repo, &["status", "--porcelain"])
        .1
        .trim()
        .is_empty());
    // 外す前に復元点を作っている
    assert!(restore_point.snapshot_ref.is_some() || restore_point.backup_ref.is_some());
    assert!(!hikae_refs(&repo).trim().is_empty());

    // 次の保存では、外したファイルは問題にならない
    write_bytes(&repo, "small.txt", 20)?;
    assert!(matches!(
        ops().save_with(&repo, "next", &options())?,
        SaveOutcome::Saved { .. }
    ));
    Ok(())
}

#[test]
fn excluding_a_tracked_file_untracks_it_without_deleting_it() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "grow.bin", 100)?;
    write_bytes(&repo, "other.txt", 10)?;
    ops().save_with(&repo, "first", &options())?;
    assert!(tracked(&repo).contains(&"grow.bin".to_string()));

    // 追跡中のファイルが大きくなった
    write_bytes(&repo, "grow.bin", 7_000)?;
    assert!(matches!(
        ops().save_with(&repo, "second", &options())?,
        SaveOutcome::NeedsSizeDecision(_)
    ));

    let mut opts = options();
    opts.exclude = vec!["grow.bin".to_string()];
    assert!(matches!(
        ops().save_with(&repo, "second", &opts)?,
        SaveOutcome::Saved { .. }
    ));
    assert!(!tracked(&repo).contains(&"grow.bin".to_string()));
    assert!(tracked(&repo).contains(&"other.txt".to_string()));
    assert_eq!(std::fs::metadata(repo.join("grow.bin"))?.len(), 7_000);
    // 過去の保存には残る
    let (_, old, _) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD~1"]);
    assert!(old.contains("grow.bin"));
    Ok(())
}

#[test]
fn exclusions_must_point_at_flagged_files_inside_the_project() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_bytes(&repo, "small.txt", 10)?;
    write_bytes(&repo, "mid.psd", 2_000)?;

    for bad in [
        "small.txt",
        "missing.bin",
        "../outside",
        ".git/config",
        "",
        ":/x",
    ] {
        let mut opts = options();
        opts.exclude = vec![bad.to_string()];
        let err = ops().save_with(&repo, "memo", &opts);
        assert!(
            matches!(err, Err(OpsError::InvalidInput(_))),
            "{bad:?} must be rejected, got {err:?}"
        );
    }
    // 失敗しても何も変更していない
    assert_eq!(get_head_commit(&repo), None);
    assert!(!repo.join(".gitignore").exists());
    assert!(hikae_refs(&repo).trim().is_empty());
    Ok(())
}

#[test]
fn deleting_or_leaving_big_tracked_files_is_not_a_problem() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    // すでに保存済みの大きいファイル（過去に閾値が違った、または外部で保存された想定）
    write_bytes(&repo, "old-big.bin", 6_000)?;
    write_bytes(&repo, "note.txt", 10)?;
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "seed"]);

    // 変更していない大きいファイルは検査の対象外
    write_bytes(&repo, "note.txt", 11)?;
    assert!(matches!(
        ops().save_with(&repo, "edit note", &options())?,
        SaveOutcome::Saved { .. }
    ));

    // 大きいファイルの削除も保存できる
    std::fs::remove_file(repo.join("old-big.bin"))?;
    assert!(matches!(
        ops().save_with(&repo, "remove big", &options())?,
        SaveOutcome::Saved { .. }
    ));
    assert!(!tracked(&repo).contains(&"old-big.bin".to_string()));
    Ok(())
}

#[test]
fn default_limits_follow_the_design_document() {
    let d = SizeLimits::default();
    assert_eq!(d.warn_bytes, 50 * 1024 * 1024);
    assert_eq!(d.block_bytes, 100 * 1024 * 1024);
    assert_eq!(SizeLimits::from_warn_mb(25).warn_bytes, 25 * 1024 * 1024);
}

// ---------------------------------------------------------------- 取り込み手順 8

struct TwoPcs {
    _tmp: tempfile::TempDir,
    bare: PathBuf,
    pc_a: PathBuf,
    pc_b: PathBuf,
}

fn two_pcs() -> Result<TwoPcs, Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    ops().init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    std::fs::write(pc_a.join("keep.txt"), "keep\n")?;
    std::fs::write(pc_a.join("secret.bin"), [0u8, 255, 1, 2, 3, 10, 13])?;
    std::fs::write(pc_a.join("gone.txt"), "will be really deleted\n")?;
    ops().save(&pc_a, "initial")?;
    ops().upload(&pc_a)?;
    ops().clone_project(&url, &pc_b, &identity("pc-b"))?;
    Ok(TwoPcs {
        _tmp: tmp,
        bare,
        pc_a,
        pc_b,
    })
}

/// PC-A が `secret.bin` を保存対象から外してアップロードする（設計書 4.8 手順 3-2 と同じ結果）
fn exclude_on_a(pcs: &TwoPcs) -> TestResult {
    let a = &pcs.pc_a;
    std::fs::write(a.join(".gitignore"), "/secret.bin\n")?;
    let (code, _, err) = run_git(a, &["rm", "--cached", "secret.bin"]);
    assert_eq!(code, 0, "{err}");
    assert!(matches!(
        ops().save(a, "exclude secret")?,
        SaveOutcome::Saved { .. }
    ));
    ops().upload(a)?;
    Ok(())
}

#[test]
fn fast_forward_restores_files_the_other_pc_stopped_tracking() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    let original = std::fs::read(b.join("secret.bin"))?;
    exclude_on_a(&pcs)?;
    assert!(
        run_git(&pcs.bare, &["ls-tree", "-r", "--name-only", "main"])
            .1
            .contains("keep.txt")
    );

    assert!(matches!(ops().pull(b)?, PullOutcome::FastForwarded));

    // 内容（バイナリ）が取り込み前のまま作業フォルダにあり、保存対象外になっている
    assert_eq!(std::fs::read(b.join("secret.bin"))?, original);
    assert!(!tracked(b).contains(&"secret.bin".to_string()));
    assert_eq!(run_git(b, &["check-ignore", "-q", "secret.bin"]).0, 0);
    assert!(run_git(b, &["status", "--porcelain"]).1.trim().is_empty());
    // 取り込みで他のファイルが壊れていない
    assert_eq!(std::fs::read_to_string(b.join("keep.txt"))?, "keep\n");
    // 取り込み前の復元点がある
    assert!(hikae_refs(b).contains("refs/hikae/backup/pull/"));
    Ok(())
}

#[test]
fn files_really_deleted_by_the_other_pc_stay_deleted() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    std::fs::remove_file(a.join("gone.txt"))?;
    ops().save(a, "delete gone")?;
    ops().upload(a)?;

    assert!(matches!(ops().pull(b)?, PullOutcome::FastForwarded));
    assert!(
        !b.join("gone.txt").exists(),
        "ignore されていない削除は書き戻さない"
    );
    assert!(b.join("secret.bin").exists());
    Ok(())
}

#[test]
fn merge_restores_files_the_other_pc_stopped_tracking() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    let original = std::fs::read(b.join("secret.bin"))?;
    // PC-B も別の保存をしている（双方に差がある）
    std::fs::write(b.join("b-only.txt"), "b\n")?;
    ops().save(b, "b change")?;
    exclude_on_a(&pcs)?;

    assert!(matches!(ops().pull(b)?, PullOutcome::Merged { .. }));

    assert_eq!(std::fs::read(b.join("secret.bin"))?, original);
    assert!(!tracked(b).contains(&"secret.bin".to_string()));
    assert!(tracked(b).contains(&"b-only.txt".to_string()));
    assert!(run_git(b, &["status", "--porcelain"]).1.trim().is_empty());
    Ok(())
}

#[test]
fn unsaved_and_untracked_files_survive_the_pull() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    exclude_on_a(&pcs)?;
    // PC-B には未保存の変更と、保存対象外の未追跡ファイルがある
    std::fs::write(b.join("draft.txt"), "draft\n")?;
    std::fs::write(b.join("keep.txt"), "edited on b\n")?;

    ops().pull(b)?;

    assert_eq!(std::fs::read_to_string(b.join("draft.txt"))?, "draft\n");
    assert_eq!(
        std::fs::read_to_string(b.join("keep.txt"))?,
        "edited on b\n"
    );
    assert!(b.join("secret.bin").exists());
    Ok(())
}

#[test]
fn a_second_pull_after_restoring_changes_nothing() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    exclude_on_a(&pcs)?;
    ops().pull(b)?;
    let head = get_head_commit(b);
    let before = std::fs::read(b.join("secret.bin"))?;

    assert!(matches!(ops().pull(b)?, PullOutcome::UpToDate));
    assert_eq!(get_head_commit(b), head);
    assert_eq!(std::fs::read(b.join("secret.bin"))?, before);
    Ok(())
}
