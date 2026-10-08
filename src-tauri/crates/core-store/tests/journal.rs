// 操作ジャーナルの統合テスト。実際の SQLite ファイルを使う。

use core_store::{redact, JournalOutcome, JournalTrigger, NewJournalEntry, Store};
use rusqlite::Connection;

fn entry(project: &str, op: &str, started: &str) -> NewJournalEntry {
    NewJournalEntry {
        project_id: project.to_string(),
        operation: op.to_string(),
        trigger: JournalTrigger::Manual,
        started_at: started.to_string(),
        finished_at: started.to_string(),
        outcome: JournalOutcome::Success,
        detail: Some("ok".to_string()),
        snapshot_ref: Some("refs/hikae/snapshots/main/20260101T000000Z".to_string()),
        backup_ref: Some("refs/hikae/backup/save/20260101T000000Z".to_string()),
        target: None,
    }
}

#[test]
fn journal_roundtrip_orders_newest_first_and_filters_by_project(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;

    store.record_journal(&entry("p1", "save", "2026-01-01T00:00:00Z"))?;
    let mut failed = entry("p1", "pull", "2026-01-02T00:00:00Z");
    failed.outcome = JournalOutcome::Failure;
    failed.trigger = JournalTrigger::Auto;
    failed.detail = Some("git".to_string());
    failed.snapshot_ref = None;
    store.record_journal(&failed)?;
    store.record_journal(&entry("p2", "restore", "2026-01-03T00:00:00Z"))?;

    let list = store.list_journal("p1", 10)?;
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].operation, "pull");
    assert_eq!(list[0].outcome, JournalOutcome::Failure);
    assert_eq!(list[0].trigger, JournalTrigger::Auto);
    assert_eq!(list[0].snapshot_ref, None);
    assert_eq!(list[1].operation, "save");
    assert_eq!(
        list[1].backup_ref.as_deref(),
        Some("refs/hikae/backup/save/20260101T000000Z")
    );

    assert_eq!(store.list_journal("p1", 1)?.len(), 1);
    assert_eq!(store.list_journal("p2", 10)?.len(), 1);
    assert!(store.list_journal("none", 10)?.is_empty());
    Ok(())
}

#[test]
fn journal_never_stores_credentials() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;

    let mut e = entry("p1", "push", "2026-01-01T00:00:00Z");
    e.detail = Some(
        "fatal: https://user:hunter2@github.com/o/r.git token=abc123 ghp_ABCDEF Authorization: Bearer xyz"
            .to_string(),
    );
    e.target = Some("x".repeat(1000));
    store.record_journal(&e)?;

    let saved = store.list_journal("p1", 1)?;
    let detail = saved[0].detail.clone().unwrap_or_default();
    for secret in ["hunter2", "abc123", "ghp_ABCDEF", "xyz"] {
        assert!(!detail.contains(secret), "leaked {secret}: {detail}");
    }
    assert!(
        detail.contains("github.com/o/r.git"),
        "host should stay: {detail}"
    );
    assert!(saved[0].target.as_deref().unwrap_or("").chars().count() <= 200);
    Ok(())
}

#[test]
fn redact_keeps_plain_text() {
    assert_eq!(redact("merged"), "merged");
    assert_eq!(redact("git-timeout"), "git-timeout");
    assert_eq!(
        redact("https://github.com/o/r.git"),
        "https://github.com/o/r.git"
    );
}

#[test]
fn migration_from_v1_database_adds_journal_and_keeps_projects(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("old.db");

    // v1 相当のデータベース（journal なし）を用意する
    {
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY, display_name TEXT NOT NULL, path TEXT NOT NULL UNIQUE,
                remote_url TEXT, owner TEXT NOT NULL, default_branch TEXT NOT NULL DEFAULT 'main',
                last_viewed_at TEXT NOT NULL, config TEXT NOT NULL DEFAULT '{}');
             INSERT INTO projects (id, display_name, path, owner, last_viewed_at)
                VALUES ('p1', 'P', '/tmp/p', 'me', '2026-01-01T00:00:00Z');
             PRAGMA user_version = 1;",
        )?;
    }

    let store = Store::open(&path)?;
    assert_eq!(store.get_project("p1")?.display_name, "P");
    store.record_journal(&entry("p1", "save", "2026-01-01T00:00:00Z"))?;
    assert_eq!(store.list_journal("p1", 10)?.len(), 1);
    drop(store);

    let conn = Connection::open(&path)?;
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    assert_eq!(version, 5);
    // 追加された列は既存の行では未設定
    let initial: Option<String> = conn.query_row(
        "SELECT initial_commit FROM projects WHERE id = 'p1'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(initial, None);

    // 再度開いても壊れない（マイグレーションは冪等）
    let store = Store::open(&path)?;
    assert_eq!(store.list_journal("p1", 10)?.len(), 1);
    Ok(())
}

#[test]
fn interrupted_operation_exposes_its_start_time_until_resolved(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    store.begin_journal(&core_store::RunningJournal {
        project_id: "p1".to_string(),
        operation: "pull".to_string(),
        trigger: JournalTrigger::Manual,
        started_at: "2026-10-07T14:30:45.700+00:00".to_string(),
        target: None,
    })?;

    // 起動時に「実行中」のまま残っていた行が「中断」になる
    let found = store.recover_interrupted("2026-10-08T00:00:00Z")?;
    assert_eq!(found.len(), 1);
    let entry = store.interrupted_operation("p1")?.ok_or("no interrupted")?;
    assert_eq!(entry.operation, "pull");
    assert_eq!(entry.outcome, JournalOutcome::Interrupted);
    // 小数秒は切り捨てた Unix 秒（2026-10-07T14:30:45Z）
    assert_eq!(entry.started_at_unix(), Some(1_791_383_445));

    // 復旧が済んだら中断の印は消える
    assert_eq!(store.resolve_interrupted("p1")?, 1);
    assert!(store.interrupted_operation("p1")?.is_none());
    Ok(())
}

#[test]
fn last_success_at_returns_the_latest_successful_run_of_one_operation(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    assert_eq!(store.last_success_at("p1", "auto-snapshot")?, None);

    store.record_journal(&entry("p1", "auto-snapshot", "2026-01-01T00:00:00Z"))?;
    store.record_journal(&entry("p1", "auto-snapshot", "2026-01-02T00:00:00Z"))?;
    // 失敗、別の操作、別のプロジェクトは数えない
    let mut failed = entry("p1", "auto-snapshot", "2026-01-03T00:00:00Z");
    failed.outcome = JournalOutcome::Failure;
    store.record_journal(&failed)?;
    store.record_journal(&entry("p1", "save", "2026-01-04T00:00:00Z"))?;
    store.record_journal(&entry("p2", "auto-snapshot", "2026-01-05T00:00:00Z"))?;

    assert_eq!(
        store.last_success_at("p1", "auto-snapshot")?.as_deref(),
        Some("2026-01-02T00:00:00Z")
    );
    assert_eq!(
        store.last_success_at("p2", "auto-snapshot")?.as_deref(),
        Some("2026-01-05T00:00:00Z")
    );
    assert_eq!(store.last_success_at("p3", "auto-snapshot")?, None);
    Ok(())
}
