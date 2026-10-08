// 操作ジャーナルの「実行中 → 完了」「中断の検出」「保持期間の整理」の統合テスト。
// 実際の SQLite ファイルを使い、時刻はすべて引数で渡す（実時間・sleep を使わない）。

use chrono::{TimeZone, Utc};
use core_store::{
    journal_cutoff, JournalFinish, JournalOutcome, JournalTrigger, NewJournalEntry, RunningJournal,
    Store,
};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn running(project: &str, op: &str, started: &str) -> RunningJournal {
    RunningJournal {
        project_id: project.to_string(),
        operation: op.to_string(),
        trigger: JournalTrigger::Manual,
        started_at: started.to_string(),
        target: None,
    }
}

fn finish(outcome: JournalOutcome, at: &str) -> JournalFinish {
    JournalFinish {
        finished_at: at.to_string(),
        outcome,
        detail: Some("saved".to_string()),
        snapshot_ref: Some("refs/hikae/snapshots/main/20260101T000000Z".to_string()),
        backup_ref: Some("refs/hikae/backup/save/20260101T000000Z".to_string()),
    }
}

fn done(project: &str, op: &str, started: &str, outcome: JournalOutcome) -> NewJournalEntry {
    NewJournalEntry {
        project_id: project.to_string(),
        operation: op.to_string(),
        trigger: JournalTrigger::Manual,
        started_at: started.to_string(),
        finished_at: started.to_string(),
        outcome,
        detail: None,
        snapshot_ref: None,
        backup_ref: None,
        target: None,
    }
}

fn open(dir: &Path) -> Result<Store, Box<dyn std::error::Error>> {
    Ok(Store::open(&dir.join("t.db"))?)
}

#[test]
fn a_running_operation_is_updated_in_place_when_it_finishes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;

    let id = store.begin_journal(&running("p1", "save", "2026-01-01T00:00:00Z"))?;
    let rows = store.list_journal("p1", 10)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, JournalOutcome::Running);
    assert_eq!(rows[0].finished_at, None);
    assert_eq!(rows[0].snapshot_ref, None);

    assert!(store.finish_journal(id, &finish(JournalOutcome::Success, "2026-01-01T00:00:05Z"))?);
    let rows = store.list_journal("p1", 10)?;
    assert_eq!(rows.len(), 1, "新しい行を足さず、同じ行を更新する");
    assert_eq!(rows[0].outcome, JournalOutcome::Success);
    assert_eq!(rows[0].finished_at.as_deref(), Some("2026-01-01T00:00:05Z"));
    assert_eq!(rows[0].detail.as_deref(), Some("saved"));
    assert_eq!(
        rows[0].backup_ref.as_deref(),
        Some("refs/hikae/backup/save/20260101T000000Z")
    );

    // 完了済みの行は二度と書き換えない
    assert!(!store.finish_journal(id, &finish(JournalOutcome::Failure, "2026-01-01T00:00:09Z"))?);
    assert_eq!(
        store.list_journal("p1", 10)?[0].outcome,
        JournalOutcome::Success
    );
    Ok(())
}

#[test]
fn finishing_never_records_running_or_interrupted_as_a_result() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;
    let id = store.begin_journal(&running("p1", "save", "2026-01-01T00:00:00Z"))?;
    store.finish_journal(id, &finish(JournalOutcome::Running, "2026-01-01T00:00:01Z"))?;
    assert_eq!(
        store.list_journal("p1", 1)?[0].outcome,
        JournalOutcome::Failure
    );
    Ok(())
}

#[test]
fn a_quiet_operation_can_be_discarded_but_finished_rows_cannot() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;
    let quiet = store.begin_journal(&running("p1", "pull", "2026-01-01T00:00:00Z"))?;
    let kept = store.begin_journal(&running("p1", "save", "2026-01-01T00:01:00Z"))?;
    store.finish_journal(
        kept,
        &finish(JournalOutcome::Success, "2026-01-01T00:01:05Z"),
    )?;

    assert!(store.discard_journal(quiet)?);
    assert!(!store.discard_journal(kept)?, "完了済みの行は消せない");
    let rows = store.list_journal("p1", 10)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation, "save");
    Ok(())
}

#[test]
fn rows_left_running_are_detected_as_interrupted_on_the_next_start() -> TestResult {
    let tmp = tempfile::tempdir()?;
    {
        // 前回の起動: save は完了、pull と resolve は完了前にアプリが終了した
        let store = open(tmp.path())?;
        let ok = store.begin_journal(&running("p1", "save", "2026-01-01T00:00:00Z"))?;
        store.finish_journal(ok, &finish(JournalOutcome::Success, "2026-01-01T00:00:02Z"))?;
        store.begin_journal(&running("p1", "pull", "2026-01-01T01:00:00Z"))?;
        store.begin_journal(&running("p2", "resolve", "2026-01-01T02:00:00Z"))?;
        store.record_journal(&done(
            "p2",
            "save",
            "2026-01-01T03:00:00Z",
            JournalOutcome::Failure,
        ))?;
    }

    // 再起動
    let store = open(tmp.path())?;
    assert!(
        store.interrupted_operation("p1")?.is_none(),
        "検出前は中断ではない"
    );
    let found = store.recover_interrupted("2026-01-02T00:00:00Z")?;
    assert_eq!(found.len(), 2);
    assert!(found
        .iter()
        .all(|e| e.outcome == JournalOutcome::Interrupted
            && e.finished_at.as_deref() == Some("2026-01-02T00:00:00Z")));
    let mut ops: Vec<_> = found.iter().map(|e| e.operation.as_str()).collect();
    ops.sort();
    assert_eq!(ops, vec!["pull", "resolve"]);

    // 状態に載る（プロジェクトごと）
    let p1 = store.interrupted_operation("p1")?.expect("p1 interrupted");
    assert_eq!(p1.operation, "pull");
    assert_eq!(
        store.interrupted_operation("p2")?.map(|e| e.operation),
        Some("resolve".to_string())
    );
    assert!(store.interrupted_operation("none")?.is_none());

    // 完了済みの行は変わらない。2 回目の検出では何も見つからない
    let p1_rows = store.list_journal("p1", 10)?;
    assert_eq!(
        p1_rows
            .iter()
            .find(|e| e.operation == "save")
            .map(|e| e.outcome),
        Some(JournalOutcome::Success)
    );
    assert!(store
        .recover_interrupted("2026-01-03T00:00:00Z")?
        .is_empty());

    // 対応が済んだら、中断の扱いは外れる（記録は残る）
    assert_eq!(store.resolve_interrupted("p1")?, 1);
    assert!(store.interrupted_operation("p1")?.is_none());
    assert!(store.interrupted_operation("p2")?.is_some());
    assert_eq!(store.list_journal("p1", 10)?.len(), 2);
    Ok(())
}

#[test]
fn purge_removes_only_old_finished_rows_and_keeps_pending_ones() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;
    store.add_project("p1", "P1", Path::new("/p1"), None, "me", "main")?;

    let now = Utc
        .with_ymd_and_hms(2026, 10, 8, 0, 0, 0)
        .single()
        .ok_or("time")?;
    let cutoff = journal_cutoff(now, 90);
    // 90 日前は 2026-07-10 00:00

    store.record_journal(&done(
        "p1",
        "save",
        "2026-01-01T00:00:00Z",
        JournalOutcome::Success,
    ))?;
    store.record_journal(&done(
        "p1",
        "pull",
        "2026-07-09T23:59:59+00:00",
        JournalOutcome::Failure,
    ))?;
    // 古い日時でも「実行中」「中断」は消さない
    store.begin_journal(&running("p1", "restore", "2026-01-02T00:00:00Z"))?;
    let pending = store.begin_journal(&running("p1", "resolve", "2026-01-03T00:00:00Z"))?;
    assert!(pending > 0);
    // 期間内
    store.record_journal(&done(
        "p1",
        "save",
        "2026-07-10T00:00:01Z",
        JournalOutcome::Success,
    ))?;
    store.record_journal(&done(
        "p1",
        "save",
        "2026-10-07T00:00:00.123456+00:00",
        JournalOutcome::Success,
    ))?;
    // 日時を読めない行は消さない
    store.record_journal(&done("p1", "save", "not a date", JournalOutcome::Success))?;
    // 登録されていないプロジェクトの行
    store.record_journal(&done(
        "gone",
        "save",
        "2026-01-01T00:00:00Z",
        JournalOutcome::Success,
    ))?;
    store.record_journal(&done(
        "gone",
        "save",
        "2026-10-01T00:00:00Z",
        JournalOutcome::Success,
    ))?;

    assert_eq!(store.purge_journal(Some("p1"), &cutoff)?, 2);
    let left: Vec<_> = store
        .list_journal("p1", 100)?
        .into_iter()
        .map(|e| (e.operation, e.outcome))
        .collect();
    assert_eq!(left.len(), 5);
    assert_eq!(
        left.iter()
            .filter(|(_, o)| *o == JournalOutcome::Running)
            .count(),
        2
    );
    assert!(left.iter().any(|(_, o)| *o == JournalOutcome::Success));

    // 他のプロジェクトの行は p1 の整理で消えない。登録外の行は None 指定で整理する
    assert_eq!(store.list_journal("gone", 10)?.len(), 2);
    assert_eq!(store.purge_journal(None, &cutoff)?, 1);
    assert_eq!(store.list_journal("gone", 10)?.len(), 1);
    // 再度実行しても同じ（冪等）
    assert_eq!(store.purge_journal(Some("p1"), &cutoff)?, 0);

    // 中断として検出された行も、古くても消えない
    store.recover_interrupted("2026-10-08T00:00:00Z")?;
    assert_eq!(store.purge_journal(Some("p1"), &journal_cutoff(now, 1))?, 1);
    let remaining = store.list_journal("p1", 100)?;
    assert_eq!(
        remaining
            .iter()
            .filter(|e| e.outcome == JournalOutcome::Interrupted)
            .count(),
        2
    );
    Ok(())
}

#[test]
fn purge_leaves_restore_point_references_of_kept_rows_intact() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;
    store.add_project("p1", "P1", Path::new("/p1"), None, "me", "main")?;
    let now = Utc
        .with_ymd_and_hms(2026, 10, 8, 0, 0, 0)
        .single()
        .ok_or("time")?;

    let old = store.begin_journal(&running("p1", "save", "2026-01-01T00:00:00Z"))?;
    store.finish_journal(
        old,
        &finish(JournalOutcome::Success, "2026-01-01T00:00:01Z"),
    )?;
    let recent = store.begin_journal(&running("p1", "save", "2026-10-01T00:00:00Z"))?;
    let mut f = finish(JournalOutcome::Success, "2026-10-01T00:00:01Z");
    f.snapshot_ref = Some("refs/hikae/snapshots/main/20261001T000000Z".to_string());
    f.backup_ref = Some("refs/hikae/backup/save/20261001T000000Z".to_string());
    store.finish_journal(recent, &f)?;

    store.purge_journal(Some("p1"), &journal_cutoff(now, 90))?;
    let rows = store.list_journal("p1", 10)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].snapshot_ref.as_deref(),
        Some("refs/hikae/snapshots/main/20261001T000000Z")
    );
    assert_eq!(
        rows[0].backup_ref.as_deref(),
        Some("refs/hikae/backup/save/20261001T000000Z")
    );
    Ok(())
}

#[test]
fn manual_refs_are_listed_for_protection_and_forgotten_after_thinning() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;

    // 手動の保存が作った復元点
    let m = store.begin_journal(&running("p1", "save", "2026-01-01T00:00:00Z"))?;
    store.finish_journal(m, &finish(JournalOutcome::Success, "2026-01-01T00:00:01Z"))?;
    // 自動実行（取り込み）が作った復元点は保護対象にしない
    let mut auto = running("p1", "pull", "2026-01-02T00:00:00Z");
    auto.trigger = JournalTrigger::Auto;
    let a = store.begin_journal(&auto)?;
    let mut f = finish(JournalOutcome::Success, "2026-01-02T00:00:01Z");
    f.snapshot_ref = Some("refs/hikae/snapshots/main/20260102T000000Z".to_string());
    f.backup_ref = Some("refs/hikae/backup/pull/20260102T000000Z".to_string());
    store.finish_journal(a, &f)?;
    // 別プロジェクト
    let o = store.begin_journal(&running("p2", "save", "2026-01-03T00:00:00Z"))?;
    store.finish_journal(o, &finish(JournalOutcome::Success, "2026-01-03T00:00:01Z"))?;

    assert_eq!(
        store.manual_restore_refs("p1")?,
        vec![
            "refs/hikae/backup/save/20260101T000000Z".to_string(),
            "refs/hikae/snapshots/main/20260101T000000Z".to_string(),
        ]
    );

    // 間引きで消えた ref は参照から外れる。他の行・他のプロジェクトは変わらない
    let gone = vec!["refs/hikae/snapshots/main/20260102T000000Z".to_string()];
    assert_eq!(store.forget_restore_refs("p1", &gone)?, 1);
    let rows = store.list_journal("p1", 10)?;
    let pull = rows.iter().find(|e| e.operation == "pull").ok_or("pull")?;
    assert_eq!(pull.snapshot_ref, None);
    assert_eq!(
        pull.backup_ref.as_deref(),
        Some("refs/hikae/backup/pull/20260102T000000Z")
    );
    assert_eq!(store.manual_restore_refs("p2")?.len(), 2);
    Ok(())
}

#[test]
fn the_journal_never_stores_credentials_even_while_running() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = open(tmp.path())?;
    let mut r = running("p1", "push", "2026-01-01T00:00:00Z");
    r.target = Some("https://user:hunter2@github.com/o/r.git token=abc123".to_string());
    let id = store.begin_journal(&r)?;
    let mut f = finish(JournalOutcome::Failure, "2026-01-01T00:00:01Z");
    f.detail = Some("fatal ghp_ABCDEF Authorization: Bearer xyz".to_string());
    store.finish_journal(id, &f)?;

    let row = &store.list_journal("p1", 1)?[0];
    let text = format!("{:?}", row);
    for secret in ["hunter2", "abc123", "ghp_ABCDEF", "xyz"] {
        assert!(!text.contains(secret), "leaked {secret}: {text}");
    }
    Ok(())
}

#[test]
fn cutoff_is_computed_from_the_given_time() {
    let now = Utc
        .with_ymd_and_hms(2026, 10, 8, 12, 30, 0)
        .single()
        .expect("time");
    let cutoff = journal_cutoff(now, 90);
    assert!(cutoff.starts_with("2026-07-10T12:30:00"), "{cutoff}");
    assert!(journal_cutoff(now, 0).starts_with("2026-10-08T12:30:00"));
}
