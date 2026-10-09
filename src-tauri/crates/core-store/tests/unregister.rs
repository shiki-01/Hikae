// 「一覧から外す」（Store::remove_project）の統合テスト。実際の SQLite ファイルを使う。
// 登録とプロジェクト別の設定だけを消し、同じフォルダを登録し直せること、操作の記録は残ることを確かめる。
// フォルダの中身・`.git`・`refs/hikae/` には、この crate は一切触れない（ファイルシステムを扱わない）。

use core_store::{JournalOutcome, JournalTrigger, NewJournalEntry, SettingsPatch, Store};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn add(store: &Store, id: &str, path: &Path) -> TestResult {
    store.add_project(
        id,
        "卒業論文",
        path,
        Some("https://github.com/o/r.git"),
        "o",
        "main",
    )?;
    Ok(())
}

#[test]
fn removing_a_project_drops_its_registration_and_overrides_but_keeps_the_folder() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    let folder = tmp.path().join("work");
    std::fs::create_dir_all(folder.join(".git"))?;
    std::fs::write(folder.join("a.txt"), "本文")?;

    add(&store, "p1", &folder)?;
    store.update_project_settings(
        "p1",
        &SettingsPatch {
            large_file_warn_mb: Some(25),
            ..SettingsPatch::default()
        },
    )?;
    assert_eq!(store.effective_settings("p1")?.large_file_warn_mb, 25);

    store.remove_project("p1")?;

    assert!(store.get_project("p1").is_err());
    assert!(store.list_projects()?.is_empty());
    // プロジェクト別の設定も消える（同じ ID で残らない）
    assert!(store.project_overrides("p1")?.keys().is_empty());
    // フォルダの中身には触れていない
    assert_eq!(std::fs::read_to_string(folder.join("a.txt"))?, "本文");
    assert!(folder.join(".git").is_dir());
    Ok(())
}

#[test]
fn the_same_folder_can_be_registered_again_with_a_new_id() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    let folder = tmp.path().join("work");
    std::fs::create_dir_all(&folder)?;

    add(&store, "p1", &folder)?;
    // 登録中は、同じフォルダを重ねて登録できない
    assert!(add(&store, "p2", &folder).is_err());

    store.remove_project("p1")?;
    add(&store, "p2", &folder)?;
    let again = store.get_project("p2")?;
    assert_eq!(again.path, folder);
    assert_eq!(store.list_projects()?.len(), 1);
    // 前の登録の設定の上書きは引き継がない
    assert_eq!(store.effective_settings("p2")?.large_file_warn_mb, 50);
    Ok(())
}

#[test]
fn the_operation_journal_survives_unregistering() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    let folder = tmp.path().join("work");
    std::fs::create_dir_all(&folder)?;
    add(&store, "p1", &folder)?;
    store.record_journal(&NewJournalEntry {
        project_id: "p1".to_string(),
        operation: "save".to_string(),
        trigger: JournalTrigger::Manual,
        started_at: "2026-01-01T00:00:00Z".to_string(),
        finished_at: "2026-01-01T00:00:01Z".to_string(),
        outcome: JournalOutcome::Success,
        detail: Some("saved".to_string()),
        snapshot_ref: None,
        backup_ref: None,
        target: None,
    })?;

    store.remove_project("p1")?;

    // 復元点の記録（どの操作がどの復元点を作ったか）は、登録を外しても消さない（設計書 6.3）
    assert_eq!(store.list_journal("p1", 10)?.len(), 1);
    Ok(())
}
