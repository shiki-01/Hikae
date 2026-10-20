// 作業コピー（設計書 4.9）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う（モックは使わない）。
// 作成・一覧・切り替え・採用、未保存の変更での拒否（何も変えない）、復元点、ぶつかり、detached にならないこと、
// 内部名の衝突（連番）を確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Identity, Ops, OpsError, PullOutcome, WorkCopyInfo};
use std::path::{Path, PathBuf};
use time::{Date, Month, OffsetDateTime, Time};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ops() -> Ops {
    // メモの末尾にトレーラーを付けない（メッセージを比べやすくする）
    Ops::new(GitRunner::from_path_env()).with_pc_name(None)
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

/// 一つのファイルを持つ保存済みのリポジトリ（ブランチは main）
fn repo_with_one_save(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = new_repo(tmp)?;
    write_test_file(&repo, "報告書.txt", "本体")?;
    ops().save(&repo, "初版")?;
    Ok(repo)
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

fn git_code(repo: &Path, args: &[&str]) -> i32 {
    run_git(repo, args).0
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| format!("<{e}>"))
}

fn current(repo: &Path) -> String {
    git_out(repo, &["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string()
}

fn branches(repo: &Path) -> Vec<String> {
    git_out(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

/// 一覧の中から、ブランチ名で 1 件を取り出す
fn find<'a>(list: &'a [WorkCopyInfo], branch: &str) -> &'a WorkCopyInfo {
    list.iter()
        .find(|w| w.branch == branch)
        .unwrap_or_else(|| panic!("{branch} not in {list:?}"))
}

/// 固定の時刻（内部名の衝突の確認用）
fn fixed_clock() -> impl Fn() -> OffsetDateTime + Send + Sync + 'static {
    move || {
        OffsetDateTime::new_utc(
            Date::from_calendar_date(2026, Month::October, 20).expect("date"),
            Time::from_hms(3, 34, 56).expect("time"),
        )
    }
}

#[test]
fn create_then_list_shows_the_work_copy_as_current_and_main_as_original() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;

    let name = ops().create_work_copy(&repo)?;

    assert!(core_git::is_work_copy_name(&name), "内部名の形: {name}");
    assert_eq!(current(&repo), name);
    let list = ops().list_work_copies(&repo)?;
    let copy = find(&list, &name);
    assert!(copy.is_current);
    assert!(!copy.is_original);
    let original = find(&list, "main");
    assert!(original.is_original);
    assert!(!original.is_current);
    // 作った作業コピーは、作成時点の HEAD と同じ版
    assert_eq!(
        git_out(&repo, &["rev-parse", &name]).trim(),
        git_out(&repo, &["rev-parse", "main"]).trim()
    );
    Ok(())
}

#[test]
fn create_is_refused_with_unsaved_changes_and_changes_nothing() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    write_test_file(&repo, "報告書.txt", "未保存の編集")?;
    write_test_file(&repo, "新しいメモ.txt", "未追跡")?;
    let head_before = get_head_commit(&repo).ok_or("no head")?;
    let branches_before = branches(&repo);

    let err = ops().create_work_copy(&repo).expect_err("must refuse");

    assert!(matches!(err, OpsError::UnsavedChanges), "{err:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(branches(&repo), branches_before);
    assert_eq!(
        get_head_commit(&repo).as_deref(),
        Some(head_before.as_str())
    );
    assert_eq!(read(&repo, "報告書.txt"), "未保存の編集");
    assert_eq!(read(&repo, "新しいメモ.txt"), "未追跡");
    Ok(())
}

#[test]
fn switch_is_refused_with_unsaved_changes_and_leaves_the_files_alone() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案の編集")?;
    ops().save(&repo, "案")?;
    // ここで未保存の変更を作る
    write_test_file(&repo, "報告書.txt", "未保存")?;

    let err = ops()
        .switch_work_copy(&repo, "main")
        .expect_err("must refuse");

    assert!(matches!(err, OpsError::UnsavedChanges), "{err:?}");
    assert_eq!(current(&repo), name);
    assert_eq!(read(&repo, "報告書.txt"), "未保存");
    Ok(())
}

#[test]
fn switching_changes_the_working_folder_to_the_other_version() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案A の本文")?;
    ops().save(&repo, "案A")?;

    ops().switch_work_copy(&repo, "main")?;
    assert_eq!(current(&repo), "main");
    assert_eq!(read(&repo, "報告書.txt"), "本体");

    ops().switch_work_copy(&repo, &name)?;
    assert_eq!(current(&repo), name);
    assert_eq!(read(&repo, "報告書.txt"), "案A の本文");
    Ok(())
}

#[test]
fn switching_to_an_unknown_name_is_rejected_before_anything_runs() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    for bad in ["wc-20261020-000000", "-b", "--orphan", "no-such-branch"] {
        let err = ops().switch_work_copy(&repo, bad).expect_err("must refuse");
        assert!(matches!(err, OpsError::InvalidInput(_)), "{bad}: {err:?}");
    }
    assert_eq!(current(&repo), "main");
    Ok(())
}

#[test]
fn checkout_failure_leaves_the_branch_and_the_files_unchanged() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    // 作業コピーに、main には無い報告書2.txt を保存する
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書2.txt", "案の版")?;
    ops().save(&repo, "案")?;
    ops().switch_work_copy(&repo, "main")?;
    // 別のアプリが操作中の状態（index.lock が残っている）を作り、checkout を git に拒否させる。
    // 未追跡ファイルは未保存の変更として先に断られるため、拒否の経路はこれで確かめる
    let lock = repo.join(".git").join("index.lock");
    std::fs::write(&lock, "")?;

    let err = ops()
        .switch_work_copy(&repo, &name)
        .expect_err("must refuse");
    std::fs::remove_file(&lock)?;

    // 既存の分類（index.lock の競合）で返る
    assert!(
        matches!(err, OpsError::IndexLocked { stale: false }),
        "{err:?}"
    );
    assert_eq!(current(&repo), "main", "ブランチは変わらない");
    assert!(!repo.join("報告書2.txt").exists(), "ファイルは変わらない");
    assert_eq!(read(&repo, "報告書.txt"), "本体");
    Ok(())
}

#[test]
fn restore_points_are_created_before_create_and_switch() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    ops().switch_work_copy(&repo, "main")?;

    let backups = git_out(
        &repo,
        &["for-each-ref", "--format=%(refname)", "refs/hikae/backup/"],
    );
    assert!(
        backups.contains("refs/hikae/backup/work-copy-create/"),
        "作る前の復元点: {backups}"
    );
    assert!(
        backups.contains("refs/hikae/backup/work-copy-switch/"),
        "切り替える前の復元点: {backups}"
    );
    // 復元点は作業コピーの作成前の HEAD（main）を指す
    let create_ref = backups
        .lines()
        .find(|l| l.contains("work-copy-create"))
        .ok_or("no create ref")?;
    assert_eq!(
        git_out(&repo, &["rev-parse", create_ref]).trim(),
        git_out(&repo, &["rev-parse", "main"]).trim()
    );
    let _ = name;
    Ok(())
}

#[test]
fn switching_and_creating_never_leave_a_detached_head() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    assert_eq!(git_code(&repo, &["symbolic-ref", "-q", "HEAD"]), 0);
    ops().switch_work_copy(&repo, "main")?;
    assert_eq!(git_code(&repo, &["symbolic-ref", "-q", "HEAD"]), 0);
    ops().switch_work_copy(&repo, &name)?;
    assert_eq!(git_code(&repo, &["symbolic-ref", "-q", "HEAD"]), 0);
    Ok(())
}

#[test]
fn adopting_without_conflict_brings_the_work_copy_into_the_original() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "追加.txt", "案で足した内容")?;
    ops().save(&repo, "案")?;
    // 元（main）の側にも、別のファイルの変更を入れて両方に差を作る
    ops().switch_work_copy(&repo, "main")?;
    write_test_file(&repo, "本体のメモ.txt", "本体で足した内容")?;
    ops().save(&repo, "本体")?;
    ops().switch_work_copy(&repo, &name)?;

    let outcome = ops().adopt_work_copy(&repo, &name, "main")?;

    assert!(matches!(outcome, PullOutcome::Merged { .. }), "{outcome:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(read(&repo, "追加.txt"), "案で足した内容");
    assert_eq!(read(&repo, "本体のメモ.txt"), "本体で足した内容");
    assert!(git_out(&repo, &["status", "--porcelain"]).trim().is_empty());
    Ok(())
}

#[test]
fn adopting_a_work_copy_that_is_only_ahead_is_a_fast_forward() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案の本文")?;
    ops().save(&repo, "案")?;

    let outcome = ops().adopt_work_copy(&repo, &name, "main")?;

    assert!(matches!(outcome, PullOutcome::FastForwarded), "{outcome:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(read(&repo, "報告書.txt"), "案の本文");
    Ok(())
}

#[test]
fn adopting_a_work_copy_with_nothing_new_is_up_to_date() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    let before = get_head_commit(&repo).ok_or("no head")?;

    let outcome = ops().adopt_work_copy(&repo, &name, "main")?;

    assert!(matches!(outcome, PullOutcome::UpToDate), "{outcome:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(get_head_commit(&repo).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn adopting_with_a_clash_returns_conflicted_and_keeps_the_merge_in_progress() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案の版")?;
    ops().save(&repo, "案")?;
    ops().switch_work_copy(&repo, "main")?;
    write_test_file(&repo, "報告書.txt", "本体の版")?;
    ops().save(&repo, "本体")?;
    ops().switch_work_copy(&repo, &name)?;

    let outcome = ops().adopt_work_copy(&repo, &name, "main")?;

    let PullOutcome::Conflicted { files } = outcome else {
        return Err(format!("expected Conflicted: {outcome:?}").into());
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "報告書.txt");
    // 解消（4.4）の流れに乗るため、取り込みの途中の状態で残る
    assert_eq!(current(&repo), "main");
    assert_eq!(
        git_code(&repo, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]),
        0
    );
    Ok(())
}

#[test]
fn adopt_rejects_names_in_the_wrong_direction() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    // 元 → 作業コピーの向き、同じ名前は拒否する
    assert!(matches!(
        ops().adopt_work_copy(&repo, "main", &name),
        Err(OpsError::InvalidInput(_))
    ));
    assert!(matches!(
        ops().adopt_work_copy(&repo, &name, &name),
        Err(OpsError::InvalidInput(_))
    ));
    assert_eq!(current(&repo), name);
    Ok(())
}

#[test]
fn adopting_is_refused_while_a_file_is_ignored_even_if_the_work_copy_changes_it() -> TestResult {
    // 無視中（skip-worktree）のファイルがあるプロジェクトでは、採用を断る（設計書 4.9 の案 A。2026-10-20）。
    // 手元の内容を退避して取り込む方法は採らない。何も変えず、無視の印も残す。
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    write_test_file(&repo, "報告書2.txt", "元の版")?;
    ops().save(&repo, "元の版を追加")?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書2.txt", "案の版")?;
    ops().save(&repo, "案")?;
    ops().switch_work_copy(&repo, "main")?;
    // 元の作業コピーで、報告書2.txt を無視中にする（手元の内容は元の版と同じ）
    ops().set_skip_worktree(&repo, &["報告書2.txt".to_string()])?;
    let head_before = get_head_commit(&repo).ok_or("no head")?;

    let err = ops()
        .adopt_work_copy(&repo, &name, "main")
        .expect_err("must refuse");

    assert!(matches!(err, OpsError::SkipWorktreePresent), "{err:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(
        get_head_commit(&repo).as_deref(),
        Some(head_before.as_str())
    );
    assert_eq!(read(&repo, "報告書2.txt"), "元の版", "手元の内容は残る");
    assert_eq!(
        ops().list_skip_worktree(&repo)?,
        vec!["報告書2.txt".to_string()],
        "無視の印は残る"
    );
    Ok(())
}

#[test]
fn adopt_is_refused_with_unsaved_changes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "未保存")?;

    let err = ops()
        .adopt_work_copy(&repo, &name, "main")
        .expect_err("must refuse");

    assert!(matches!(err, OpsError::UnsavedChanges), "{err:?}");
    assert_eq!(current(&repo), name);
    assert_eq!(read(&repo, "報告書.txt"), "未保存");
    Ok(())
}

#[test]
fn snapshot_refs_of_a_work_copy_have_no_slash_in_the_branch_part() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案の編集")?;
    // 作業コピーの上で保存すると、自動の復元点（スナップショット）も作られる
    ops().save(&repo, "案")?;
    ops().switch_work_copy(&repo, "main")?;

    let refs = git_out(
        &repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hikae/snapshots/",
        ],
    );
    let prefix = format!("refs/hikae/snapshots/{name}/");
    let mine: Vec<&str> = refs.lines().filter(|r| r.starts_with(&prefix)).collect();
    assert!(!mine.is_empty(), "作業コピーの復元点: {refs}");
    for r in &mine {
        // 接頭辞の後ろは時刻 1 つだけ（ブランチ名が / を含まない）
        let rest = &r[prefix.len()..];
        assert!(!rest.contains('/'), "{r}");
    }
    Ok(())
}

#[test]
fn colliding_names_in_the_same_second_get_a_numbered_suffix() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = repo_with_one_save(tmp.path())?;
    let clock = Ops::new(GitRunner::from_path_env())
        .with_pc_name(None)
        .with_clock(fixed_clock());

    let first = clock.create_work_copy(&repo)?;
    assert_eq!(first, "wc-20261020-033456");
    // 同じ秒にもう一度作ると連番が付く（作業コピーの内容は同じ版から作られる）
    let second = clock.create_work_copy(&repo)?;
    assert_eq!(second, "wc-20261020-033456-1");
    let third = clock.create_work_copy(&repo)?;
    assert_eq!(third, "wc-20261020-033456-2");
    assert_eq!(current(&repo), third);
    Ok(())
}

#[test]
fn create_without_any_saved_version_is_rejected() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;

    let err = ops().create_work_copy(&repo).expect_err("must refuse");

    assert!(matches!(err, OpsError::InvalidInput(_)), "{err:?}");
    assert_eq!(current(&repo), "main");
    Ok(())
}

/// 無視中（skip-worktree）のファイルに手元の編集を残して、main にいる状態を作る。
/// 作業コピーは一つ作って保存済み（案の版は作業コピー側だけにある）。
fn repo_with_skipped_local_edit(
    tmp: &Path,
) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let repo = repo_with_one_save(tmp)?;
    let name = ops().create_work_copy(&repo)?;
    write_test_file(&repo, "報告書.txt", "案の編集")?;
    ops().save(&repo, "案")?;
    ops().switch_work_copy(&repo, "main")?;
    // 無視中にしてから、手元だけで編集する（作業フォルダの内容は main の版と違う）
    ops().set_skip_worktree(&repo, &["報告書.txt".to_string()])?;
    write_test_file(&repo, "報告書.txt", "手元の編集")?;
    Ok((repo, name))
}

/// 無視中のファイルの印が付いているか（`ls-files -v` の先頭が `S`）
fn is_skipped(repo: &Path, rel: &str) -> bool {
    git_out(repo, &["ls-files", "-v", "--", rel]).starts_with("S ")
}

#[test]
fn switch_is_refused_while_a_file_is_ignored_and_changes_nothing() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, name) = repo_with_skipped_local_edit(tmp.path())?;
    let head_before = get_head_commit(&repo).ok_or("no head")?;

    let err = ops()
        .switch_work_copy(&repo, &name)
        .expect_err("must refuse");

    assert!(matches!(err, OpsError::SkipWorktreePresent), "{err:?}");
    assert_eq!(err.kind(), "skip-worktree-present");
    assert_eq!(current(&repo), "main");
    assert_eq!(
        get_head_commit(&repo).as_deref(),
        Some(head_before.as_str())
    );
    // 手元の内容と無視の印は、そのまま残る
    assert_eq!(read(&repo, "報告書.txt"), "手元の編集");
    assert!(is_skipped(&repo, "報告書.txt"));
    Ok(())
}

#[test]
fn adopt_is_refused_while_a_file_is_ignored_and_changes_nothing() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, name) = repo_with_skipped_local_edit(tmp.path())?;
    let head_before = get_head_commit(&repo).ok_or("no head")?;

    let err = ops()
        .adopt_work_copy(&repo, &name, "main")
        .expect_err("must refuse");

    assert!(matches!(err, OpsError::SkipWorktreePresent), "{err:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(
        get_head_commit(&repo).as_deref(),
        Some(head_before.as_str())
    );
    assert_eq!(read(&repo, "報告書.txt"), "手元の編集");
    assert!(is_skipped(&repo, "報告書.txt"));
    Ok(())
}

#[test]
fn create_is_allowed_while_a_file_is_ignored_and_keeps_the_local_copy() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, _) = repo_with_skipped_local_edit(tmp.path())?;

    // 作業フォルダは変わらない（同じ版から枝を作って切り替えるだけ）
    let created = ops().create_work_copy(&repo)?;

    assert_eq!(current(&repo), created);
    assert_eq!(read(&repo, "報告書.txt"), "手元の編集");
    assert!(is_skipped(&repo, "報告書.txt"));
    Ok(())
}

#[test]
fn switch_works_again_once_the_ignore_is_cleared() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, name) = repo_with_skipped_local_edit(tmp.path())?;
    // 無視を外すと、手元の編集は未保存の変更として見える。先に保存してから切り替える
    ops().clear_skip_worktree(&repo, &["報告書.txt".to_string()])?;
    ops().save(&repo, "手元の編集")?;

    ops().switch_work_copy(&repo, &name)?;

    assert_eq!(current(&repo), name);
    assert_eq!(read(&repo, "報告書.txt"), "案の編集");
    Ok(())
}
