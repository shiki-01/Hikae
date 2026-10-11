// 中断された操作の判定（設計書 6.3）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 「途中で止まった」状態は、ジャーナルの行を残す代わりに git を直接呼んで作る（復元点を作らずに止めるため）。
// 時刻は固定の clock で与え、時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Identity, Ops, OpsError, RecoverDiagnosis, RecoverOutcome};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 復旧テストで使う固定の時刻（Unix 秒）。セットアップで作る実時刻の復元点はこれより前になる
const FIXED_UNIX: i64 = 1_800_000_000;

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn ops_named(pc: &str) -> Ops {
    Ops::new(GitRunner::from_path_env()).with_pc_name(Some(pc.to_string()))
}

fn ops_fixed(pc: &str) -> Ops {
    ops_named(pc).with_clock(|| {
        OffsetDateTime::from_unix_timestamp(FIXED_UNIX).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    })
}

struct Pair {
    _tmp: tempfile::TempDir,
    pc_a: PathBuf,
    pc_b: PathBuf,
}

/// PC-A が保存して upload し、PC-B が取得した状態（復元点はどちらも実時刻で作られている）
fn pair() -> Result<Pair, Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    let a = ops_named("PC-A");
    a.init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    std::fs::write(pc_a.join("keep.txt"), "keep v1\n")?;
    a.save(&pc_a, "initial")?;
    a.upload(&pc_a)?;
    ops_named("PC-B").clone_project(&url, &pc_b, &identity("pc-b"))?;
    Ok(Pair {
        _tmp: tmp,
        pc_a,
        pc_b,
    })
}

/// PC-A が別のファイルを保存して upload する（PC-B は取り込んでいない）
fn upload_change_from_a(pc_a: &Path, name: &str) -> TestResult {
    std::fs::write(pc_a.join(name), format!("{name}\n"))?;
    let a = ops_named("PC-A");
    a.save(pc_a, "A change")?;
    a.upload(pc_a)?;
    Ok(())
}

/// 作業フォルダ・インデックス・refs・HEAD の状態。何も変わっていないことを比べるために使う
fn state_of(repo: &Path) -> Vec<String> {
    vec![
        run_git(repo, &["status", "--porcelain=v2"]).1,
        run_git(repo, &["ls-files", "-s"]).1,
        run_git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]).1,
        run_git(repo, &["rev-parse", "HEAD"]).1,
        std::fs::read_to_string(repo.join("keep.txt")).unwrap_or_default(),
    ]
}

fn expect_restored(outcome: RecoverOutcome) -> Result<String, Box<dyn std::error::Error>> {
    match outcome {
        RecoverOutcome::Restored { restore_point } => Ok(restore_point),
        RecoverOutcome::NothingChanged => Err("復元点へ戻すはずだった".into()),
    }
}

fn expect_not_found(result: Result<RecoverOutcome, OpsError>) -> RecoverDiagnosis {
    match result {
        Err(OpsError::RestorePointNotFound(diagnosis)) => diagnosis,
        other => panic!("RestorePointNotFound を期待した: {other:?}"),
    }
}

// ---------------------------------------------------------------- (1) 何も起きていない

#[test]
fn an_interrupted_fetch_with_no_restore_point_is_closed_without_changes() -> TestResult {
    let p = pair()?;
    upload_change_from_a(&p.pc_a, "a-new.txt")?;
    // 取り込みは fetch の途中で止まった（復元点は作られず、HEAD も動いていない）
    assert_eq!(run_git(&p.pc_b, &["fetch", "origin"]).0, 0);
    let head = get_head_commit(&p.pc_b).ok_or("no head")?;
    std::fs::write(p.pc_b.join("draft.txt"), "unsaved\n")?;
    let before = state_of(&p.pc_b);

    let ops = ops_fixed("PC-B");
    let check = ops.check_interrupted(&p.pc_b, FIXED_UNIX, Some(&head))?;
    assert!(check.no_change_proven(), "{:?}", check.diagnosis);
    assert_eq!(check.diagnosis.head_changed(), Some(false));

    let outcome = ops.recover_interrupted(&p.pc_b, "pull", FIXED_UNIX, Some(&head))?;
    assert_eq!(outcome, RecoverOutcome::NothingChanged);
    // 判定と確定は、作業フォルダ・インデックス・refs・HEAD を変えない（未保存の変更も残る）
    assert_eq!(state_of(&p.pc_b), before);
    assert!(!p.pc_b.join("a-new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(p.pc_b.join("draft.txt"))?,
        "unsaved\n"
    );
    Ok(())
}

// ---------------------------------------------------------------- (2) HEAD が開始時と違う

#[test]
fn a_head_that_moved_without_a_restore_point_is_not_closed_and_is_explained() -> TestResult {
    let p = pair()?;
    upload_change_from_a(&p.pc_a, "a-new.txt")?;
    assert_eq!(run_git(&p.pc_b, &["fetch", "origin"]).0, 0);
    let head_before = get_head_commit(&p.pc_b).ok_or("no head")?;
    // 復元点を作らずに HEAD だけ進める（開始後に履歴が変わった状態）
    assert_eq!(run_git(&p.pc_b, &["merge", "--ff-only", "@{u}"]).0, 0);
    let head_now = get_head_commit(&p.pc_b).ok_or("no head")?;
    assert_ne!(head_before, head_now);

    let ops = ops_fixed("PC-B");
    let check = ops.check_interrupted(&p.pc_b, FIXED_UNIX, Some(&head_before))?;
    assert!(!check.no_change_proven());

    let diagnosis =
        expect_not_found(ops.recover_interrupted(&p.pc_b, "pull", FIXED_UNIX, Some(&head_before)));
    assert_eq!(diagnosis.head_changed(), Some(true));
    let line = diagnosis.technical_line("pull", Some(240), "2026-10-07T14:30:45+00:00");
    assert!(line.contains("head_changed=true"), "{line}");
    assert!(line.contains("backup_refs_after_start=0"), "{line}");
    assert!(
        line.contains(&format!("head_before={}", &head_before[..12])),
        "{line}"
    );
    assert!(
        line.contains(&format!("head_now={}", &head_now[..12])),
        "{line}"
    );
    assert!(line.contains("merge_head=false"), "{line}");
    // 失敗しても何も変えない
    assert_eq!(get_head_commit(&p.pc_b).as_deref(), Some(head_now.as_str()));
    Ok(())
}

// ---------------------------------------------------------------- (3) 取り込みの途中（MERGE_HEAD）

#[test]
fn a_half_finished_merge_is_never_closed_automatically() -> TestResult {
    let p = pair()?;
    upload_change_from_a(&p.pc_a, "a-new.txt")?;
    // PC-B は別の変更を保存してから、取り込みを途中（MERGE_HEAD が残った状態）で止める
    std::fs::write(p.pc_b.join("b-local.txt"), "local\n")?;
    ops_named("PC-B").save(&p.pc_b, "B local")?;
    assert_eq!(run_git(&p.pc_b, &["fetch", "origin"]).0, 0);
    let head_before = get_head_commit(&p.pc_b).ok_or("no head")?;
    assert_eq!(
        run_git(&p.pc_b, &["merge", "--no-commit", "--no-ff", "@{u}"]).0,
        0,
        "競合のない取り込みを作る"
    );
    assert_eq!(
        run_git(&p.pc_b, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).0,
        0
    );
    let before = state_of(&p.pc_b);

    let ops = ops_fixed("PC-B");
    let check = ops.check_interrupted(&p.pc_b, FIXED_UNIX, Some(&head_before))?;
    assert!(
        !check.no_change_proven(),
        "MERGE_HEAD があるのに何も無いとしてはならない"
    );
    assert!(check.diagnosis.merge_head);

    // 復旧は取り込みを取り消す（通常どおりの復元点から戻す）。自動確定ではない
    let diagnosis =
        expect_not_found(ops.recover_interrupted(&p.pc_b, "pull", FIXED_UNIX, Some(&head_before)));
    assert!(diagnosis.merge_head);
    assert_eq!(state_of(&p.pc_b), before, "失敗した呼び出しは何も変えない");
    Ok(())
}

// ---------------------------------------------------------------- (4) 開始時の HEAD の記録が無い

#[test]
fn a_row_without_a_recorded_head_is_never_closed_automatically() -> TestResult {
    let p = pair()?;
    upload_change_from_a(&p.pc_a, "a-new.txt")?;
    assert_eq!(run_git(&p.pc_b, &["fetch", "origin"]).0, 0);
    let before = state_of(&p.pc_b);

    let ops = ops_fixed("PC-B");
    let check = ops.check_interrupted(&p.pc_b, FIXED_UNIX, None)?;
    assert!(!check.no_change_proven());
    assert_eq!(check.diagnosis.head_changed(), None);

    let diagnosis = expect_not_found(ops.recover_interrupted(&p.pc_b, "pull", FIXED_UNIX, None));
    assert_eq!(diagnosis.head_before, None);
    let line = diagnosis.technical_line("pull", None, "2026-10-07T14:30:45+00:00");
    assert!(line.contains("head_changed=unknown"), "{line}");
    assert!(line.contains("head_before=none"), "{line}");
    assert_eq!(state_of(&p.pc_b), before);
    Ok(())
}

// ---------------------------------------------------------------- (5) 復元点がある

#[test]
fn a_restore_point_takes_precedence_and_the_recovery_restores_it() -> TestResult {
    let p = pair()?;
    upload_change_from_a(&p.pc_a, "a-new.txt")?;
    // 取り込みは最後まで行われた（開始後の復元点 pull/... ができている）
    let head_before = get_head_commit(&p.pc_b).ok_or("no head")?;
    assert!(matches!(
        ops_fixed("PC-B").pull(&p.pc_b)?,
        core_ops::PullOutcome::FastForwarded
    ));
    let check = ops_fixed("PC-B").check_interrupted(&p.pc_b, FIXED_UNIX, Some(&head_before))?;
    assert!(!check.no_change_proven());
    assert!(check.diagnosis.backup_refs_after_start >= 1);

    let restored = expect_restored(ops_fixed("PC-B").recover_interrupted(
        &p.pc_b,
        "pull",
        FIXED_UNIX,
        Some(&head_before),
    )?)?;
    assert!(
        restored.starts_with("refs/hikae/backup/pull/"),
        "{restored}"
    );
    assert!(!p.pc_b.join("a-new.txt").exists());
    Ok(())
}

// ---------------------------------------------------------------- 補助: 履歴が無いリポジトリ

#[test]
fn current_head_is_none_for_a_repository_without_commits() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("empty");
    std::fs::create_dir_all(&repo)?;
    assert_eq!(run_git(&repo, &["init"]).0, 0);
    assert_eq!(ops_named("PC-X").current_head(&repo)?, None);
    Ok(())
}
