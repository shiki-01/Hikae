// 拡張機能の記録（`extensions` テーブル）の統合テスト。実際の SQLite ファイルを使う（設計書 9.1、11.9）。
// 行が無い拡張は有効、有効・無効の切り替えは版を含めて更新、取り外しは記録だけを消すことを確かめる。

use core_store::{ExtensionState, Store};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn state(id: &str, version: &str, enabled: bool) -> ExtensionState {
    ExtensionState {
        id: id.to_string(),
        version: version.to_string(),
        enabled,
    }
}

#[test]
fn fresh_database_has_no_extension_records() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    assert!(store.list_extension_states()?.is_empty());
    Ok(())
}

#[test]
fn set_enabled_inserts_then_updates_version_and_flag() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;

    store.set_extension_enabled("example.tool", "1.0.0", true)?;
    assert_eq!(
        store.list_extension_states()?,
        vec![state("example.tool", "1.0.0", true)]
    );

    // 同じ ID の再記録は行を増やさず、版と有効の値を更新する
    store.set_extension_enabled("example.tool", "1.1.0", false)?;
    assert_eq!(
        store.list_extension_states()?,
        vec![state("example.tool", "1.1.0", false)]
    );
    Ok(())
}

#[test]
fn list_is_sorted_by_id_and_survives_reopen() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("t.db");
    {
        let store = Store::open(&path)?;
        store.set_extension_enabled("zeta.tool", "1.0.0", true)?;
        store.set_extension_enabled("alpha.tool", "2.0.0", false)?;
    }
    let store = Store::open(&path)?;
    assert_eq!(
        store.list_extension_states()?,
        vec![
            state("alpha.tool", "2.0.0", false),
            state("zeta.tool", "1.0.0", true),
        ]
    );
    Ok(())
}

#[test]
fn remove_drops_only_the_given_record_and_missing_id_is_ok() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = Store::open(&tmp.path().join("t.db"))?;
    store.set_extension_enabled("a.one", "1.0.0", true)?;
    store.set_extension_enabled("b.two", "1.0.0", false)?;

    store.remove_extension_state("a.one")?;
    assert_eq!(
        store.list_extension_states()?,
        vec![state("b.two", "1.0.0", false)]
    );

    // 無い ID を消しても失敗しない
    store.remove_extension_state("no.such")?;
    assert_eq!(store.list_extension_states()?.len(), 1);
    Ok(())
}
