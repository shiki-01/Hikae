// 設定の保存・読み出し・プロジェクト別上書きの統合テスト。実際の SQLite ファイルを使う。

use core_store::{
    AppSettings, ConflictMode, ProjectConfig, SettingsPatch, Store, StoreError,
    PROJECT_OVERRIDABLE_KEYS,
};
use rusqlite::Connection;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn store_with_project(dir: &Path) -> Result<Store, Box<dyn std::error::Error>> {
    let store = Store::open(&dir.join("t.db"))?;
    store.add_project("p1", "論文", &dir.join("p1"), None, "alice", "main")?;
    store.add_project("p2", "家計簿", &dir.join("p2"), None, "alice", "main")?;
    Ok(store)
}

#[test]
fn fresh_database_returns_chapter_7_defaults() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = store_with_project(tmp.path())?;
    let settings = store.get_settings()?;
    assert_eq!(settings, AppSettings::default());
    assert_eq!(settings.pull_interval_minutes, 15);
    assert_eq!(settings.conflict_mode, ConflictMode::NotifyOnly);
    assert_eq!(store.effective_settings("p1")?, settings);
    // 旧 ProjectConfig の既定値も設計書と一致する
    assert_eq!(
        ProjectConfig::default().resolve_conflict_mode.as_deref(),
        Some("notify-only")
    );
    Ok(())
}

#[test]
fn update_persists_across_reopen_and_changes_only_given_fields() -> TestResult {
    let tmp = tempfile::tempdir()?;
    {
        let store = store_with_project(tmp.path())?;
        let after = store.update_settings(&SettingsPatch {
            pull_interval_minutes: Some(5),
            auto_push_after_save: Some(false),
            onboarded: Some(true),
            ..SettingsPatch::default()
        })?;
        assert_eq!(after.pull_interval_minutes, 5);
        assert!(!after.auto_push_after_save);
        assert!(after.onboarded);
        // 指定していない項目は既定値のまま
        assert!(after.pull_on_startup);
    }
    let reopened = Store::open(&tmp.path().join("t.db"))?;
    let s = reopened.get_settings()?;
    assert_eq!(s.pull_interval_minutes, 5);
    assert!(!s.auto_push_after_save);
    assert!(s.onboarded);

    // 2 回目の更新は 1 回目の値を保つ
    reopened.update_settings(&SettingsPatch {
        conflict_mode: Some(ConflictMode::ShowDialog),
        ..SettingsPatch::default()
    })?;
    let s = reopened.get_settings()?;
    assert_eq!(s.conflict_mode, ConflictMode::ShowDialog);
    assert_eq!(s.pull_interval_minutes, 5);
    Ok(())
}

#[test]
fn invalid_values_are_rejected_and_nothing_is_saved() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = store_with_project(tmp.path())?;
    let result = store.update_settings(&SettingsPatch {
        auto_push_after_save: Some(false),
        pull_interval_minutes: Some(1441),
        ..SettingsPatch::default()
    });
    assert!(matches!(result, Err(StoreError::InvalidData(_))));
    // 同じ更新に含まれていた有効な項目も保存されない
    assert!(store.get_settings()?.auto_push_after_save);
    Ok(())
}

#[test]
fn project_overrides_layer_on_top_of_global_settings() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = store_with_project(tmp.path())?;
    store.update_settings(&SettingsPatch {
        pull_interval_minutes: Some(60),
        ..SettingsPatch::default()
    })?;

    let overrides = store.update_project_settings(
        "p1",
        &SettingsPatch {
            pull_interval_minutes: Some(5),
            save_before_pull: Some(false),
            ..SettingsPatch::default()
        },
    )?;
    assert_eq!(overrides.pull_interval_minutes, Some(5));
    assert_eq!(overrides.save_before_pull, Some(false));
    assert_eq!(overrides.auto_push_after_save, None);

    let p1 = store.effective_settings("p1")?;
    assert_eq!(p1.pull_interval_minutes, 5);
    assert!(!p1.save_before_pull);
    assert!(p1.auto_push_after_save);
    // 別のプロジェクトは全体設定のまま
    assert_eq!(store.effective_settings("p2")?.pull_interval_minutes, 60);
    assert!(store.effective_settings("p2")?.save_before_pull);

    // 全体設定を変えても、上書きは優先される
    store.update_settings(&SettingsPatch {
        pull_interval_minutes: Some(0),
        ..SettingsPatch::default()
    })?;
    assert_eq!(store.effective_settings("p1")?.pull_interval_minutes, 5);
    assert_eq!(store.effective_settings("p2")?.pull_interval_minutes, 0);
    Ok(())
}

#[test]
fn global_only_keys_cannot_be_overridden_per_project() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = store_with_project(tmp.path())?;
    for patch in [
        SettingsPatch {
            git_lfs: Some(true),
            ..SettingsPatch::default()
        },
        SettingsPatch {
            onboarded: Some(true),
            ..SettingsPatch::default()
        },
    ] {
        let result = store.update_project_settings("p1", &patch);
        assert!(matches!(result, Err(StoreError::InvalidData(_))));
    }
    assert!(!store.effective_settings("p1")?.git_lfs);
    assert!(PROJECT_OVERRIDABLE_KEYS.contains(&"pull_interval_minutes"));

    // 存在しないプロジェクトへは書けない
    let result = store.update_project_settings(
        "missing",
        &SettingsPatch {
            pull_interval_minutes: Some(5),
            ..SettingsPatch::default()
        },
    );
    assert!(matches!(result, Err(StoreError::ProjectNotFound(_))));
    Ok(())
}

#[test]
fn removing_a_project_removes_its_overrides() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let store = store_with_project(tmp.path())?;
    store.update_project_settings(
        "p1",
        &SettingsPatch {
            pull_interval_minutes: Some(5),
            ..SettingsPatch::default()
        },
    )?;
    store.remove_project("p1")?;
    // 同じ ID で登録し直しても、古い上書きは残っていない
    store.add_project("p1", "論文", &tmp.path().join("p1"), None, "alice", "main")?;
    assert_eq!(store.effective_settings("p1")?.pull_interval_minutes, 15);
    Ok(())
}

#[test]
fn corrupted_stored_values_fall_back_to_defaults() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let db = tmp.path().join("t.db");
    {
        let store = Store::open(&db)?;
        store.add_project("p1", "論文", &tmp.path().join("p1"), None, "alice", "main")?;
    }
    // アプリ外でデータベースが書き換えられた状況を作る
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        "INSERT INTO settings (key, value) VALUES
            ('pull_interval_minutes', '9999'),
            ('conflict_mode', '\"explode\"'),
            ('auto_push_after_save', 'not json'),
            ('unknown_key', 'true'),
            ('pull_on_startup', 'false');
         INSERT INTO project_settings (project_id, key, value) VALUES
            ('p1', 'git_lfs', 'true'),
            ('p1', 'large_file_warn_mb', '100');",
    )?;
    drop(conn);

    let store = Store::open(&db)?;
    let s = store.get_settings()?;
    assert_eq!(s.pull_interval_minutes, 15);
    assert_eq!(s.conflict_mode, ConflictMode::NotifyOnly);
    assert!(s.auto_push_after_save);
    assert!(!s.pull_on_startup);
    // 上書きできないキーの上書きは無視され、有効なものだけ反映される
    let effective = store.effective_settings("p1")?;
    assert!(!effective.git_lfs);
    assert_eq!(effective.large_file_warn_mb, 100);
    Ok(())
}

#[test]
fn legacy_project_config_is_migrated_into_project_settings() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let db = tmp.path().join("t.db");
    {
        let store = Store::open(&db)?;
        store.add_project("p1", "論文", &tmp.path().join("p1"), None, "alice", "main")?;
        store.add_project(
            "p2",
            "家計簿",
            &tmp.path().join("p2"),
            None,
            "alice",
            "main",
        )?;
    }
    // v3 までの形式（projects.config の JSON に上書きがある）へ戻す
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        "DROP TABLE settings;
         DROP TABLE project_settings;
         PRAGMA user_version = 3;
         UPDATE projects SET config =
            '{\"pull_interval_minutes\":5,\"auto_push_after_save\":false,\"resolve_conflict_mode\":\"show-dialog\",\"auto_save_before_pull\":null,\"snapshot_retention_days\":1234}'
            WHERE id = 'p1';
         UPDATE projects SET config = 'not json' WHERE id = 'p2';",
    )?;
    drop(conn);

    let store = Store::open(&db)?;
    let p1 = store.effective_settings("p1")?;
    assert_eq!(p1.pull_interval_minutes, 5);
    assert!(!p1.auto_push_after_save);
    assert_eq!(p1.conflict_mode, ConflictMode::ShowDialog);
    // null と選択肢外の値は取り込まれず、既定値のまま
    assert!(p1.save_before_pull);
    assert_eq!(p1.snapshot_retention_days, 90);
    // 壊れた JSON のプロジェクトは上書きなし
    assert_eq!(store.effective_settings("p2")?, AppSettings::default());
    Ok(())
}
