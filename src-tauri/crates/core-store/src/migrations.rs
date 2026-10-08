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
        assert_eq!(version, 3);
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
