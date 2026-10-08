// データベースマイグレーション（user_version で管理）。

use crate::StoreError;
use rusqlite::Connection;

/// すべてのマイグレーションを実行する。
pub fn run_migrations(conn: &Connection) -> Result<(), StoreError> {
    let current_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if current_version < 1 {
        migrate_to_v1(conn)?;
    }
    if current_version < 2 {
        migrate_to_v2(conn)?;
    }
    if current_version < 3 {
        migrate_to_v3(conn)?;
    }
    if current_version < 4 {
        migrate_to_v4(conn)?;
    }

    // 将来のバージョンはここに追加

    Ok(())
}

/// v1: 初期スキーマ
/// - projects: プロジェクト登録情報
/// - snapshots: スナップショット索引（Phase 2）
/// - skip_worktree: skip-worktree 設定記録（Phase 2）
/// - llm_models: LLM モデル管理（Phase 2）
/// - message_cache: 生成済みメモキャッシュ（Phase 2）
/// - extensions: 拡張機能（Phase 3）
fn migrate_to_v1(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            remote_url TEXT,
            owner TEXT NOT NULL,
            default_branch TEXT NOT NULL DEFAULT 'main',
            last_viewed_at TEXT NOT NULL,
            config TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE IF NOT EXISTS snapshots (
            project_id TEXT NOT NULL,
            ref_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            reason TEXT,
            changed_files_count INTEGER,
            PRIMARY KEY (project_id, ref_name),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS skip_worktree (
            project_id TEXT NOT NULL,
            path TEXT NOT NULL,
            set_at TEXT NOT NULL,
            PRIMARY KEY (project_id, path),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS llm_models (
            id TEXT PRIMARY KEY,
            stage INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            sha256 TEXT,
            size_bytes INTEGER,
            installed_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS message_cache (
            tree_oid TEXT NOT NULL,
            model_id TEXT NOT NULL,
            generated_message TEXT NOT NULL,
            PRIMARY KEY (tree_oid, model_id),
            FOREIGN KEY (model_id) REFERENCES llm_models(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS extensions (
            id TEXT PRIMARY KEY,
            version TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            permissions TEXT NOT NULL DEFAULT '[]'
        );

        PRAGMA user_version = 1;
        "#,
    )?;

    Ok(())
}

/// v2: 操作ジャーナル（設計書 6.3）
/// 外部キーは張らない。プロジェクトの登録を外しても操作の記録は残す。
fn migrate_to_v2(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS journal (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            operation TEXT NOT NULL,
            triggered_by TEXT NOT NULL DEFAULT 'manual',
            started_at TEXT NOT NULL,
            finished_at TEXT NOT NULL,
            outcome TEXT NOT NULL,
            detail TEXT,
            snapshot_ref TEXT,
            backup_ref TEXT,
            target TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_journal_project_started
            ON journal (project_id, started_at DESC, id DESC);

        PRAGMA user_version = 2;
        "#,
    )?;

    Ok(())
}

/// v3: プロジェクトに最初の保存（初期 commit）の OID を記録する列を追加。
/// 保存先 URL が無いプロジェクトでも、フォルダの付け替え時に同じ履歴か照合できるようにする。
/// 既存の行は NULL のまま（アプリが次に履歴を読めたときに補う）。
fn migrate_to_v3(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        ALTER TABLE projects ADD COLUMN initial_commit TEXT;

        PRAGMA user_version = 3;
        "#,
    )?;

    Ok(())
}

/// v4: 設定（設計書 7章）。全体の `settings` とプロジェクト別の上書き `project_settings`。
/// 旧 `projects.config`（JSON）に入っていた上書きは、有効な値だけ `project_settings` へ移す。
/// 移行後も `projects.config` の列は残すが、設定の読み書きには使わない。
fn migrate_to_v4(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS project_settings (
            project_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (project_id, key),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        "#,
    )?;

    let legacy: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, config FROM projects")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (project_id, config) in legacy {
        for (key, value) in legacy_overrides(&config) {
            conn.execute(
                "INSERT OR IGNORE INTO project_settings (project_id, key, value) VALUES (?, ?, ?)",
                rusqlite::params![project_id, key, value],
            )?;
        }
    }

    conn.execute_batch("PRAGMA user_version = 4;")?;
    Ok(())
}

/// 旧 `ProjectConfig` の JSON から、新しい設定キーと JSON 値の組を取り出す。
/// 壊れた JSON、null、選択肢にない値は取り込まない。
fn legacy_overrides(config: &str) -> Vec<(String, String)> {
    const RENAMES: &[(&str, &str)] = &[
        ("auto_pull_on_startup", "pull_on_startup"),
        ("pull_interval_minutes", "pull_interval_minutes"),
        ("auto_save_before_pull", "save_before_pull"),
        ("resolve_conflict_mode", "conflict_mode"),
        ("auto_push_after_save", "auto_push_after_save"),
        ("push_notification_interval_hours", "push_reminder_hours"),
        ("auto_save_snapshots", "auto_snapshot_enabled"),
        ("snapshot_retention_days", "snapshot_retention_days"),
    ];
    let Ok(serde_json::Value::Object(old)) = serde_json::from_str::<serde_json::Value>(config)
    else {
        return Vec::new();
    };
    let candidates = RENAMES.iter().filter_map(|(from, to)| {
        let value = old.get(*from).filter(|v| !v.is_null())?;
        Some((to.to_string(), value.to_string()))
    });
    crate::settings::valid_stored_entries(
        candidates,
        Some(crate::settings::PROJECT_OVERRIDABLE_KEYS),
    )
    .into_iter()
    .map(|(k, v)| (k, v.to_string()))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_adds_nullable_initial_commit_and_keeps_existing_rows() {
        let conn = Connection::open_in_memory().expect("open");
        // v2 までのデータベースを作り、既存のプロジェクトを 1 件入れる
        migrate_to_v1(&conn).expect("v1");
        migrate_to_v2(&conn).expect("v2");
        conn.execute(
            "INSERT INTO projects (id, display_name, path, remote_url, owner, default_branch, last_viewed_at, config)
             VALUES ('p1', 'name', '/p', NULL, 'o', 'main', '2026-01-01T00:00:00Z', '{}')",
            [],
        )
        .expect("insert");

        run_migrations(&conn).expect("migrate");

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("version");
        assert_eq!(version, 4);
        let initial: Option<String> = conn
            .query_row(
                "SELECT initial_commit FROM projects WHERE id = 'p1'",
                [],
                |r| r.get(0),
            )
            .expect("select");
        assert_eq!(initial, None);

        // 2 回目の実行は何も変えない
        run_migrations(&conn).expect("rerun");
    }
}
