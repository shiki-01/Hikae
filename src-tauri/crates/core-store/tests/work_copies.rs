// 作業コピーの記録（設計書 4.9、migration v9）の統合テスト。実際の SQLite ファイルを使う。
// 追加・一覧・しまう・プロジェクトの登録を外したときの片付けを確かめる。
// Git 側のブランチには触れない（この crate はファイルシステムと Git を扱わない）。

use core_store::Store;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn add_project(store: &Store, id: &str, path: &Path) -> TestResult {
    store.add_project(id, "卒業論文", path, None, "o", "main")?;
    Ok(())
}

#[test]
fn work_copies_are_listed_in_creation_order_per_project() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    add_project(&store, "p1", &tmp.path().join("a"))?;
    add_project(&store, "p2", &tmp.path().join("b"))?;

    store.add_work_copy("p1", "wc-20261020-120001", "案B", "2026-10-20T03:00:01Z")?;
    store.add_work_copy("p1", "wc-20261020-120000", "案A", "2026-10-20T03:00:00Z")?;
    store.add_work_copy(
        "p2",
        "wc-20261020-120000",
        "別のプロジェクト",
        "2026-10-20T03:00:00Z",
    )?;

    let rows = store.list_work_copies("p1")?;
    let names: Vec<&str> = rows.iter().map(|r| r.branch.as_str()).collect();
    assert_eq!(names, ["wc-20261020-120000", "wc-20261020-120001"]);
    assert_eq!(rows[0].display_name, "案A");
    assert!(!rows[0].archived);
    // 同じ内部名は別のプロジェクトでも、同じプロジェクトでは重複させない
    assert_eq!(store.list_work_copies("p2")?.len(), 1);
    assert!(store
        .add_work_copy("p1", "wc-20261020-120000", "重複", "2026-10-20T03:00:02Z")
        .is_err());
    Ok(())
}

#[test]
fn archiving_hides_nothing_in_the_store_and_can_be_undone() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    add_project(&store, "p1", &tmp.path().join("a"))?;
    store.add_work_copy("p1", "wc-20261020-120000", "案A", "2026-10-20T03:00:00Z")?;

    assert!(store.set_work_copy_archived("p1", "wc-20261020-120000", true)?);
    let rows = store.list_work_copies("p1")?;
    assert_eq!(rows.len(), 1, "しまっても行は消えない");
    assert!(rows[0].archived);

    assert!(store.set_work_copy_archived("p1", "wc-20261020-120000", false)?);
    assert!(!store.list_work_copies("p1")?[0].archived);
    // 記録が無い作業コピーは何も変えない
    assert!(!store.set_work_copy_archived("p1", "wc-20261020-999999", true)?);
    Ok(())
}

#[test]
fn removing_a_project_drops_its_work_copy_records() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    let folder = tmp.path().join("work");
    std::fs::create_dir_all(&folder)?;
    add_project(&store, "p1", &folder)?;
    store.add_work_copy("p1", "wc-20261020-120000", "案A", "2026-10-20T03:00:00Z")?;

    store.remove_project("p1")?;

    // 同じフォルダを新しい ID で登録し直しても、古い作業コピーの記録は見えない
    add_project(&store, "p2", &folder)?;
    assert!(store.list_work_copies("p1")?.is_empty());
    assert!(store.list_work_copies("p2")?.is_empty());
    Ok(())
}
