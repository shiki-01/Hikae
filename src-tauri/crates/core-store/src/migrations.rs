// データベースマイグレーション（user_version で管理）。

use crate::StoreError;
use rusqlite::Connection;

/// すべてのマイグレーションを実行する。
pub fn run_migrations(conn: &Connection) -> Result<(), StoreError> {
    let current_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if current_version == 0 {
        migrate_to_v1(conn)?;
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
