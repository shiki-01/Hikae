// 「今後の変更だけ無視する」（skip-worktree、設計書 4.8）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリ（bare リモート＋2 台の PC）を使う。モックは使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Choice, Identity, Ops, OpsError, PullOutcome, SaveOutcome};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use time::OffsetDateTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 1 秒ずつ進む時刻（復元点の名前が衝突しないように）
fn ops_with_clock(clock: Arc<AtomicU64>) -> Ops {
    Ops::new(GitRunner::from_path_env()).with_clock(move || {
        let nanos = clock.fetch_add(1_000_000_000, Ordering::SeqCst);
        OffsetDateTime::from_unix_timestamp_nanos(nanos as i128)
            .unwrap_or(OffsetDateTime::now_utc())
    })
}

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

/// bare リモートと、PC-A（保存・アップロードする側）、PC-B（取り込む側）
struct Pair {
    _tmp: tempfile::TempDir,
    pc_a: PathBuf,
    pc_b: PathBuf,
    ops_a: Ops,
    ops_b: Ops,
}

fn setup(name: &str) -> Result<Pair, Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new().prefix(name).tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    let ops_a = ops_with_clock(Arc::new(AtomicU64::new(1_800_000_000_000_000_000)));
    ops_a.init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    write_test_file(&pc_a, "big.psd", "ORIGINAL")?;
    write_test_file(&pc_a, "other.txt", "v1")?;
    ops_a.save(&pc_a, "initial")?;
    ops_a.upload(&pc_a)?;

    let ops_b = ops_with_clock(Arc::new(AtomicU64::new(1_900_000_000_000_000_000)));
    ops_b.clone_project(&url, &pc_b, &identity("pc-b"))?;

    Ok(Pair {
        _tmp: tmp,
        pc_a,
        pc_b,
        ops_a,
        ops_b,
    })
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).expect("read file")
}

/// `git` の標準出力（失敗すると panic）
fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

/// `ls-files -v` で、そのパスが無視中（`S`）か
fn is_skipped(repo: &Path, rel: &str) -> bool {
    let (_, stdout, _) = run_git(repo, &["ls-files", "-v", "-z", "--", rel]);
    stdout
        .split('\0')
        .any(|e| e.starts_with("S ") && e.ends_with(rel))
}

/// 退避フォルダの一覧
fn stash_dirs(repo: &Path) -> Vec<PathBuf> {
    let root = repo.join(".git").join("hikae-skip-stash");
    match std::fs::read_dir(&root) {
        Ok(read) => read.map(|e| e.expect("entry").path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// 退避の内容（1 件だけある前提）
fn only_stash_content(repo: &Path) -> String {
    let dirs = stash_dirs(repo);
    assert_eq!(dirs.len(), 1, "expected exactly one saved copy");
    std::fs::read_to_string(dirs[0].join("content")).expect("stash content")
}

/// 指定の接頭辞の ref の数
fn ref_count(repo: &Path, prefix: &str) -> usize {
    git_out(repo, &["for-each-ref", "--format=%(refname)", prefix])
        .lines()
        .filter(|l| !l.is_empty())
        .count()
}

/// PC-B で big.psd を書き換えて、アップロードする
fn cloud_changes_big_psd(pair: &Pair, content: &str) -> TestResult {
    write_test_file(&pair.pc_b, "big.psd", content)?;
    pair.ops_b.save(&pair.pc_b, "cloud edit")?;
    pair.ops_b.upload(&pair.pc_b)?;
    Ok(())
}

#[test]
fn set_requires_a_tracked_file_and_makes_a_restore_point() -> TestResult {
    let pair = setup("skip-set")?;
    let repo = &pair.pc_a;

    assert!(pair
        .ops_a
        .set_skip_worktree(repo, &["missing.psd".to_string()])
        .is_err_and(|e| matches!(e, OpsError::InvalidInput(_))));
    assert_eq!(ref_count(repo, "refs/hikae/backup/skip-worktree-set/"), 0);

    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(ref_count(repo, "refs/hikae/backup/skip-worktree-set/"), 1);
    // 作業フォルダのファイルは変えない
    assert_eq!(read(repo, "big.psd"), "ORIGINAL");
    assert_eq!(
        pair.ops_a.list_skip_worktree(repo)?,
        vec!["big.psd".to_string()]
    );

    // 無視中のものを指定し直しても、復元点は増えない
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    assert_eq!(ref_count(repo, "refs/hikae/backup/skip-worktree-set/"), 1);
    Ok(())
}

#[test]
fn local_edits_to_an_ignored_file_do_not_appear_or_get_saved() -> TestResult {
    let pair = setup("skip-save")?;
    let repo = &pair.pc_a;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;

    // 変更として現れない（一覧にも、保存の対象にも）
    let changes = pair.ops_a.list_changes(repo)?;
    assert!(changes.iter().all(|c| c.path != "big.psd"), "{changes:?}");
    assert!(matches!(
        pair.ops_a.save(repo, "only the ignored file changed")?,
        SaveOutcome::NothingToSave
    ));

    // ほかのファイルを保存しても、無視中のファイルの内容は入らない
    write_test_file(repo, "other.txt", "v2")?;
    assert!(matches!(
        pair.ops_a.save(repo, "other")?,
        SaveOutcome::Saved { .. }
    ));
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "ORIGINAL");
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    Ok(())
}

#[test]
fn clearing_the_ignore_makes_local_edits_visible_again() -> TestResult {
    let pair = setup("skip-clear")?;
    let repo = &pair.pc_a;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    assert!(pair
        .ops_a
        .list_changes(repo)?
        .iter()
        .all(|c| c.path != "big.psd"));

    pair.ops_a
        .clear_skip_worktree(repo, &["big.psd".to_string()])?;
    assert!(!is_skipped(repo, "big.psd"));
    assert_eq!(ref_count(repo, "refs/hikae/backup/skip-worktree-clear/"), 1);
    let changes = pair.ops_a.list_changes(repo)?;
    assert!(changes.iter().any(|c| c.path == "big.psd"), "{changes:?}");
    // 作業フォルダの内容は変わっていない
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");

    // 無視中でないもの・追跡されていないものは何もしない（復元点も作らない）
    pair.ops_a
        .clear_skip_worktree(repo, &["big.psd".to_string(), "nothing.psd".to_string()])?;
    assert_eq!(ref_count(repo, "refs/hikae/backup/skip-worktree-clear/"), 1);
    Ok(())
}

#[test]
fn restoring_an_earlier_save_keeps_the_ignore_and_the_local_copy_is_in_the_restore_point(
) -> TestResult {
    let pair = setup("skip-restore")?;
    let repo = &pair.pc_a;
    let first = git_out(repo, &["rev-parse", "HEAD"]).trim().to_string();
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    write_test_file(repo, "other.txt", "v2")?;
    pair.ops_a.save(repo, "second")?;

    // 確認画面の件数（作業フォルダと対象の差）には、無視中のファイルは出ない
    let preview = pair.ops_a.restore_preview(repo, &first)?;
    let listed: Vec<&String> = preview
        .modified
        .iter()
        .map(|c| &c.path)
        .chain(preview.deleted.iter())
        .chain(preview.created.iter())
        .collect();
    assert!(listed.iter().all(|p| p.as_str() != "big.psd"), "{listed:?}");

    // 以前の保存へ戻す。restore --staged は印を外すため、戻した後に印を付け直していること
    let token = pair
        .ops_a
        .restore(repo, &first)?
        .expect("restore point token");
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "ORIGINAL");
    assert_eq!(read(repo, "other.txt"), "v1");
    // 無視中のファイルの手元の内容は、復元の対象外（そのまま残る）
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    // 戻す前の手元の内容は、復元点（スナップショット）のどれかに残っている。
    // 直前の保存と同じ内容だったため、戻す操作は新しいスナップショットを作らず、HEAD の控えを指す
    assert!(
        token.starts_with("refs/hikae/backup/") || token.starts_with("refs/hikae/snapshots/"),
        "{token}"
    );
    let snapshots = git_out(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hikae/snapshots/",
        ],
    );
    let kept = snapshots
        .lines()
        .any(|r| git_out(repo, &["show", &format!("{r}:big.psd")]) == "LOCAL SETTINGS");
    assert!(kept, "the local copy is not in any restore point");

    // 戻した後に保存しても、無視中のファイルの内容は入らない
    write_test_file(repo, "other.txt", "v3")?;
    pair.ops_a.save(repo, "third")?;
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "ORIGINAL");
    assert_eq!(git_out(repo, &["diff", "--cached", "--name-only"]), "");
    Ok(())
}

#[test]
fn pull_keeps_the_local_copy_when_the_cloud_changes_the_ignored_file() -> TestResult {
    let pair = setup("skip-pull")?;
    pair.ops_a
        .set_skip_worktree(&pair.pc_a, &["big.psd".to_string()])?;
    write_test_file(&pair.pc_a, "big.psd", "LOCAL SETTINGS")?;
    cloud_changes_big_psd(&pair, "CLOUD")?;

    let outcome = pair.ops_a.pull(&pair.pc_a)?;
    assert!(matches!(outcome, PullOutcome::FastForwarded), "{outcome:?}");
    // 手元の内容は作業フォルダに戻り、履歴には相手の版が入る。印は付け直されている
    assert_eq!(read(&pair.pc_a, "big.psd"), "LOCAL SETTINGS");
    assert_eq!(git_out(&pair.pc_a, &["show", "HEAD:big.psd"]), "CLOUD");
    assert!(is_skipped(&pair.pc_a, "big.psd"));
    assert!(stash_dirs(&pair.pc_a).is_empty());
    assert!(pair.ops_a.take_skip_report().is_empty());
    assert!(pair
        .ops_a
        .list_changes(&pair.pc_a)?
        .iter()
        .all(|c| c.path != "big.psd"));
    Ok(())
}

#[test]
fn pull_keeps_the_local_copy_and_reports_when_the_cloud_deletes_the_ignored_file() -> TestResult {
    let pair = setup("skip-pull-delete")?;
    let repo = &pair.pc_a;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;

    std::fs::remove_file(pair.pc_b.join("big.psd"))?;
    pair.ops_b.save(&pair.pc_b, "cloud removed")?;
    pair.ops_b.upload(&pair.pc_b)?;

    pair.ops_a.pull(repo)?;
    let report = pair.ops_a.take_skip_report();
    assert_eq!(report.removed_upstream, vec!["big.psd".to_string()]);
    assert!(report.pending.is_empty());
    // 書き戻さない（作業フォルダに無く、インデックスにも無い）。内容は退避に残る
    assert!(!repo.join("big.psd").exists());
    assert!(run_git(repo, &["ls-files", "--", "big.psd"]).1.is_empty());
    assert_eq!(only_stash_content(repo), "LOCAL SETTINGS");
    Ok(())
}

#[test]
fn conflict_keeps_the_copy_until_resolved_and_then_restores_the_ignore() -> TestResult {
    let pair = setup("skip-conflict")?;
    let repo = &pair.pc_a;
    // PC-A は自分の保存を作ってから無視にし、手元で編集する。PC-B も同じファイルを変える
    write_test_file(repo, "big.psd", "A COMMITTED")?;
    pair.ops_a.save(repo, "a edit")?;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    cloud_changes_big_psd(&pair, "B CLOUD")?;

    let outcome = pair.ops_a.pull(repo)?;
    assert!(
        matches!(outcome, PullOutcome::Conflicted { .. }),
        "{outcome:?}"
    );
    assert_eq!(only_stash_content(repo), "LOCAL SETTINGS");
    assert!(pair.ops_a.take_skip_report().is_empty());

    // 解消（この PC の版を採用）→ 退避を書き戻し、印を付け直す
    pair.ops_a.resolve(
        repo,
        &[("big.psd".to_string(), Choice::Mine)],
        &[],
        false,
        "merged",
    )?;
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "A COMMITTED");
    assert!(stash_dirs(repo).is_empty());
    Ok(())
}

#[test]
fn abort_after_a_conflict_restores_the_ignore_and_the_local_copy() -> TestResult {
    let pair = setup("skip-abort")?;
    let repo = &pair.pc_a;
    write_test_file(repo, "big.psd", "A COMMITTED")?;
    pair.ops_a.save(repo, "a edit")?;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    cloud_changes_big_psd(&pair, "B CLOUD")?;
    assert!(matches!(
        pair.ops_a.pull(repo)?,
        PullOutcome::Conflicted { .. }
    ));

    pair.ops_a.abort_merge(repo)?;
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "A COMMITTED");
    assert!(stash_dirs(repo).is_empty());
    Ok(())
}

#[test]
fn a_failure_before_the_copy_changes_nothing() -> TestResult {
    let pair = setup("skip-stash-fail")?;
    let repo = &pair.pc_a;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    cloud_changes_big_psd(&pair, "CLOUD")?;

    // 退避の置き場所（.git/hikae-skip-stash）をフォルダとして作れない状態にする。
    // Windows でも確実に失敗させるため、同名のファイルを置く
    std::fs::write(repo.join(".git").join("hikae-skip-stash"), "blocked")?;
    assert!(pair.ops_a.pull(repo).is_err());

    // 取り込みは行われていない。印も、手元の内容も、履歴も元のまま
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "ORIGINAL");
    Ok(())
}

#[test]
fn recover_after_an_interrupted_pull_restores_the_ignore() -> TestResult {
    let pair = setup("skip-recover")?;
    let repo = &pair.pc_a;
    write_test_file(repo, "big.psd", "A COMMITTED")?;
    pair.ops_a.save(repo, "a edit")?;
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    write_test_file(repo, "big.psd", "LOCAL SETTINGS")?;
    cloud_changes_big_psd(&pair, "B CLOUD")?;
    assert!(matches!(
        pair.ops_a.pull(repo)?,
        PullOutcome::Conflicted { .. }
    ));
    // 解消される前に（アプリが終了した状況を想定して）復旧する
    pair.ops_a.recover_interrupted(repo, "pull", 0)?;

    assert!(run_git(repo, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"]).0 != 0);
    assert!(is_skipped(repo, "big.psd"));
    assert_eq!(read(repo, "big.psd"), "LOCAL SETTINGS");
    assert_eq!(git_out(repo, &["show", "HEAD:big.psd"]), "A COMMITTED");
    assert!(stash_dirs(repo).is_empty());
    Ok(())
}

#[test]
fn a_copy_the_user_edited_meanwhile_is_kept_and_reported_as_pending() -> TestResult {
    let pair = setup("skip-pending")?;
    let repo = &pair.pc_a;
    // 退避が残っていて、利用者が作業フォルダのファイルをさらに編集した状態を作る
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    pair.ops_a
        .clear_skip_worktree(repo, &["big.psd".to_string()])?;
    let stash = repo.join(".git").join("hikae-skip-stash").join("pending-1");
    std::fs::create_dir_all(&stash)?;
    std::fs::write(stash.join("content"), "OLD LOCAL")?;
    std::fs::write(stash.join("meta.json"), r#"{"path":"big.psd"}"#)?;
    write_test_file(repo, "big.psd", "EDITED AFTER")?;

    let report = pair.ops_a.reapply_skip_worktree(repo)?;
    assert_eq!(report.pending, vec!["big.psd".to_string()]);
    assert!(report.removed_upstream.is_empty());
    // 上書きしない・印も付けない・退避は残す
    assert_eq!(read(repo, "big.psd"), "EDITED AFTER");
    assert!(!is_skipped(repo, "big.psd"));
    assert!(stash.join("content").exists());

    // 退避が残っている間は、同じファイルの無視を指定できない（古い内容を書き戻さないため）
    assert!(pair
        .ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])
        .is_err_and(|e| matches!(e, OpsError::SkipStashPending)));
    Ok(())
}

#[test]
fn drift_lists_recorded_tracked_files_whose_ignore_was_lost() -> TestResult {
    let pair = setup("skip-drift")?;
    let repo = &pair.pc_a;
    let recorded = vec!["big.psd".to_string(), "gone.psd".to_string()];
    // 無視していない（記録だけある）ものは、外れている扱い。追跡されていないものは対象外
    assert_eq!(
        pair.ops_a.list_skip_worktree_drift(repo, &recorded)?,
        vec!["big.psd".to_string()]
    );
    pair.ops_a
        .set_skip_worktree(repo, &["big.psd".to_string()])?;
    assert!(pair
        .ops_a
        .list_skip_worktree_drift(repo, &recorded)?
        .is_empty());
    Ok(())
}

#[test]
fn set_and_clear_reject_paths_outside_the_project_and_empty_lists() -> TestResult {
    let pair = setup("skip-invalid")?;
    let repo = &pair.pc_a;
    for bad in ["../outside.psd", "/abs.psd", ".git/config", ""] {
        assert!(
            pair.ops_a
                .set_skip_worktree(repo, &[bad.to_string()])
                .is_err(),
            "set accepted {bad:?}"
        );
        assert!(
            pair.ops_a
                .clear_skip_worktree(repo, &[bad.to_string()])
                .is_err(),
            "clear accepted {bad:?}"
        );
    }
    assert!(pair.ops_a.set_skip_worktree(repo, &[]).is_err());
    Ok(())
}

#[test]
fn the_runner_rejects_update_index_forms_outside_the_allowlist() {
    let runner = GitRunner::from_path_env();
    let repo = std::env::temp_dir();
    for args in [
        vec![
            "update-index",
            "--cacheinfo",
            "100644,0000000000000000000000000000000000000000,x",
        ],
        vec!["update-index", "--assume-unchanged", "--", "a"],
        vec!["update-index", "--add", "--", "a"],
        vec!["update-index", "--force-remove", "--", "a"],
        vec!["update-index", "--skip-worktree", "a"],
        vec!["update-index", "--skip-worktree", "--"],
    ] {
        let result = runner.run(&repo, &args);
        assert!(
            matches!(result, Err(core_git::GitError::Rejected(_))),
            "not rejected: {args:?}"
        );
    }
}
