// 自動保存のクラウド控え（設計書 6.4）の統合テスト。実 git と、一時ディレクトリの bare リポジトリを
// 保存先（origin）にした実リポジトリを使う。
// 確かめること: 送られる（snapshots の ref だけ）、2 回目は何もしない、refs/hikae/backup は送らない、
// ローカルで ref を消してもリモートに残る、ref が 0 件なら何もしない、100 件を超えても全部送る、
// 送れないとき（保存先が無い）はエラーになる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{CloudBackupOutcome, Identity, Ops, SizeLimits, CLOUD_BACKUP_BATCH};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SNAPSHOTS: &str = "refs/hikae/snapshots/";
const BACKUPS: &str = "refs/hikae/backup/";

struct Setup {
    _tmp: tempfile::TempDir,
    work: PathBuf,
    bare: PathBuf,
    ops: Ops,
}

fn identity() -> Identity {
    Identity {
        name: "tester".to_string(),
        email: "tester@users.noreply.github.com".to_string(),
    }
}

/// 保存先（origin）を持つ実リポジトリを作る。まだ保存はしていない
fn setup(name: &str) -> Result<Setup, Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new().prefix(name).tempdir()?;
    let work = tmp.path().join("work");
    let bare = tmp.path().join("remote.git");
    create_bare_repo(&bare)?;
    let ops = Ops::new(GitRunner::from_path_env());
    let url = bare.to_string_lossy().to_string();
    ops.init_project(&work, Some(&url), &identity())?;
    Ok(Setup {
        _tmp: tmp,
        work,
        bare,
        ops,
    })
}

/// 指定した接頭辞の ref 名の一覧（ソート済み）
fn refs_under(repo: &Path, prefix: &str) -> Vec<String> {
    let (_, out, _) = run_git(repo, &["for-each-ref", "--format=%(refname)", prefix]);
    let mut refs: Vec<String> = out.lines().map(str::to_string).collect();
    refs.sort();
    refs
}

/// 未保存の変更を作って、自動保存を 1 件作る
fn take_auto_snapshot(s: &Setup, content: &str) -> TestResult {
    write_test_file(&s.work, "memo.txt", content)?;
    let outcome = s.ops.auto_snapshot(&s.work, SizeLimits::default())?;
    assert!(
        matches!(outcome, core_ops::AutoSnapshotOutcome::Created { .. }),
        "auto snapshot should be created: {outcome:?}"
    );
    Ok(())
}

#[test]
fn sends_snapshots_only_and_second_run_is_a_no_op() -> TestResult {
    let s = setup("cloud-backup-send")?;
    // 保存を 2 回して、復元点（スナップショットと backup）を両方作る
    write_test_file(&s.work, "a.txt", "1")?;
    s.ops.save(&s.work, "最初")?;
    write_test_file(&s.work, "a.txt", "2")?;
    s.ops.save(&s.work, "次")?;
    take_auto_snapshot(&s, "自動保存 1")?;
    take_auto_snapshot(&s, "自動保存 2")?;

    let local_snapshots = refs_under(&s.work, SNAPSHOTS);
    assert!(local_snapshots.len() >= 2, "{local_snapshots:?}");
    assert!(
        !refs_under(&s.work, BACKUPS).is_empty(),
        "backup refs exist locally"
    );

    let first = s.ops.backup_snapshots_to_cloud(&s.work)?;
    assert_eq!(
        first,
        CloudBackupOutcome {
            pushed_refs: local_snapshots.len() as u32,
            total_refs: local_snapshots.len() as u32,
        }
    );
    // 保存先には自動保存の ref だけが届く（backup は送らない）
    assert_eq!(refs_under(&s.bare, SNAPSHOTS), local_snapshots);
    assert!(refs_under(&s.bare, BACKUPS).is_empty());
    assert_eq!(refs_under(&s.bare, "refs/hikae/"), local_snapshots);

    // 2 回目は送り済みなので、新しく作った ref は 0 件
    let second = s.ops.backup_snapshots_to_cloud(&s.work)?;
    assert_eq!(second.pushed_refs, 0);
    assert_eq!(second.total_refs, local_snapshots.len() as u32);
    assert_eq!(refs_under(&s.bare, SNAPSHOTS), local_snapshots);
    Ok(())
}

#[test]
fn local_deletion_does_not_remove_the_cloud_copy() -> TestResult {
    let s = setup("cloud-backup-keep")?;
    write_test_file(&s.work, "a.txt", "1")?;
    s.ops.save(&s.work, "最初")?;
    take_auto_snapshot(&s, "自動保存")?;
    s.ops.backup_snapshots_to_cloud(&s.work)?;
    let before = refs_under(&s.bare, SNAPSHOTS);
    assert!(!before.is_empty());

    // ローカルの ref を消す（間引きの相当）。`update-ref -d` は自アプリの名前空間の削除
    let victim = refs_under(&s.work, SNAPSHOTS)
        .into_iter()
        .next()
        .expect("a local snapshot");
    let (code, _, _) = run_git(&s.work, &["update-ref", "-d", &victim]);
    assert_eq!(code, 0);
    assert!(!refs_under(&s.work, SNAPSHOTS).contains(&victim));

    // 次の控えでも、リモート側の ref は消えない
    let outcome = s.ops.backup_snapshots_to_cloud(&s.work)?;
    assert_eq!(outcome.pushed_refs, 0);
    assert_eq!(refs_under(&s.bare, SNAPSHOTS), before);
    assert!(refs_under(&s.bare, SNAPSHOTS).contains(&victim));
    Ok(())
}

#[test]
fn no_snapshot_refs_is_a_no_op() -> TestResult {
    let s = setup("cloud-backup-empty")?;
    assert!(refs_under(&s.work, "refs/hikae/").is_empty());

    let outcome = s.ops.backup_snapshots_to_cloud(&s.work)?;
    assert_eq!(
        outcome,
        CloudBackupOutcome {
            pushed_refs: 0,
            total_refs: 0
        }
    );
    assert!(refs_under(&s.bare, "refs/hikae/").is_empty());
    Ok(())
}

#[test]
fn sends_more_than_one_batch() -> TestResult {
    let s = setup("cloud-backup-batch")?;
    write_test_file(&s.work, "a.txt", "1")?;
    s.ops.save(&s.work, "最初")?;
    let (_, head, _) = run_git(&s.work, &["rev-parse", "HEAD"]);
    let head = head.trim().to_string();

    // 100 件を超える ref を、同じ commit を指して作る
    let count = CLOUD_BACKUP_BATCH + 5;
    for i in 0..count {
        let name = format!("{SNAPSHOTS}main/t{i:04}");
        let (code, _, stderr) = run_git(&s.work, &["update-ref", &name, &head]);
        assert_eq!(code, 0, "{stderr}");
    }
    // 保存が作った自動保存の 1 件を含めて、100 件を超える
    let local = refs_under(&s.work, SNAPSHOTS);
    assert!(local.len() > CLOUD_BACKUP_BATCH, "{}", local.len());

    let outcome = s.ops.backup_snapshots_to_cloud(&s.work)?;
    assert_eq!(outcome.total_refs as usize, local.len());
    assert_eq!(outcome.pushed_refs as usize, local.len());
    assert_eq!(refs_under(&s.bare, SNAPSHOTS), local);
    Ok(())
}

#[test]
fn unreachable_remote_is_an_error() -> TestResult {
    let s = setup("cloud-backup-offline")?;
    write_test_file(&s.work, "a.txt", "1")?;
    s.ops.save(&s.work, "最初")?;
    take_auto_snapshot(&s, "自動保存")?;
    // 保存先を存在しない場所へ向ける（オフラインに相当）
    let missing = s.bare.with_file_name("missing.git");
    let (code, _, _) = run_git(
        &s.work,
        &["remote", "set-url", "origin", &missing.to_string_lossy()],
    );
    assert_eq!(code, 0);

    assert!(s.ops.backup_snapshots_to_cloud(&s.work).is_err());
    // 失敗しても、ローカルの ref は変えない
    assert!(!refs_under(&s.work, SNAPSHOTS).is_empty());
    Ok(())
}
