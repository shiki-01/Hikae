// アプリ内データ管理（SQLite: rusqlite）。
// プロジェクト登録、スナップショット索引、設定を管理する。

use rusqlite::{params, Connection, Result as SqliteResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub mod journal;
pub mod migrations;
pub mod models;
pub mod settings;

pub use journal::{
    journal_cutoff, journal_cutoff_from_now, now_rfc3339, redact, JournalEntry, JournalFinish,
    JournalOutcome, JournalTrigger, NewJournalEntry, RunningJournal,
};
pub use migrations::run_migrations;
pub use models::{Project, ProjectConfig};
pub use settings::{
    AiModelSource, AiRunner, AiScope, AppSettings, ConflictMode, DefaultVisibility, GitExecutable,
    MemoSuggestion, OpenAction, SettingsPatch, TermDisplay, TimelineSnapshots,
    PROJECT_OVERRIDABLE_KEYS,
};

/// Store のエラー型
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// SQLite データベースへのアクセスをラップする。
/// すべてのテーブルスキーマはマイグレーション時に user_version で管理される。
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 既存の、またはこれから作成するデータベースに接続する。
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        // 親ディレクトリがなければ作成する
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;

        // マイグレーション実行
        run_migrations(&conn)?;

        Ok(Store { conn })
    }

    /// プロジェクト一覧を取得。最終表示日時でソート。
    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name, path, remote_url, owner, default_branch, last_viewed_at, config, initial_commit
             FROM projects
             ORDER BY last_viewed_at DESC"
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    path: row.get::<_, String>(2)?.into(),
                    remote_url: row.get(3)?,
                    owner: row.get(4)?,
                    default_branch: row.get(5)?,
                    last_viewed_at: row.get(6)?,
                    config: row.get(7)?,
                    initial_commit: row.get(8)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(projects)
    }

    /// ID でプロジェクトを取得。
    pub fn get_project(&self, id: &str) -> Result<Project, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name, path, remote_url, owner, default_branch, last_viewed_at, config, initial_commit
             FROM projects WHERE id = ?"
        )?;

        let project = stmt
            .query_row([id], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    path: row.get::<_, String>(2)?.into(),
                    remote_url: row.get(3)?,
                    owner: row.get(4)?,
                    default_branch: row.get(5)?,
                    last_viewed_at: row.get(6)?,
                    config: row.get(7)?,
                    initial_commit: row.get(8)?,
                })
            })
            .map_err(|_| StoreError::ProjectNotFound(id.to_string()))?;

        Ok(project)
    }

    /// プロジェクトを追加。
    pub fn add_project(
        &self,
        id: &str,
        display_name: &str,
        path: &Path,
        remote_url: Option<&str>,
        owner: &str,
        default_branch: &str,
    ) -> Result<(), StoreError> {
        let now = chrono::Utc::now().to_rfc3339();
        let config = serde_json::json!({});

        self.conn.execute(
            "INSERT INTO projects (id, display_name, path, remote_url, owner, default_branch, last_viewed_at, config)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                display_name,
                path.to_string_lossy().to_string(),
                remote_url,
                owner,
                default_branch,
                now,
                config.to_string(),
            ],
        )?;

        Ok(())
    }

    /// プロジェクトを更新（表示名、最終表示日時など）。
    pub fn update_project(
        &self,
        id: &str,
        display_name: Option<&str>,
        remote_url: Option<Option<&str>>,
        owner: Option<&str>,
        config: Option<&str>,
    ) -> Result<(), StoreError> {
        // 現在の値を取得
        let current = self.get_project(id)?;

        let new_display_name = display_name.unwrap_or(&current.display_name);
        let new_remote_url = remote_url
            .map(|u| u.map(|s| s.to_string()))
            .unwrap_or(current.remote_url);
        let new_owner = owner.unwrap_or(&current.owner);
        let new_config = config.unwrap_or(&current.config);
        let now = chrono::Utc::now().to_rfc3339();

        self.conn.execute(
            "UPDATE projects
             SET display_name = ?, remote_url = ?, owner = ?, config = ?, last_viewed_at = ?
             WHERE id = ?",
            params![
                new_display_name,
                new_remote_url,
                new_owner,
                new_config,
                now,
                id,
            ],
        )?;

        Ok(())
    }

    /// プロジェクトを削除（登録のみ。フォルダは消さない）。
    pub fn remove_project(&self, id: &str) -> Result<(), StoreError> {
        // 外部キーの連鎖削除に頼らず、プロジェクト別の設定も明示的に消す
        self.conn.execute(
            "DELETE FROM project_settings WHERE project_id = ?",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM projects WHERE id = ?", params![id])?;
        Ok(())
    }

    // ===== 設定（設計書 7章・7.1） =====

    fn read_entries(&self, sql: &str, args: &[&str]) -> Result<Vec<(String, String)>, StoreError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// 全体設定を取得する。保存されていない項目や、壊れた値の項目は既定値になる。
    pub fn get_settings(&self) -> Result<AppSettings, StoreError> {
        let rows = self.read_entries("SELECT key, value FROM settings", &[])?;
        Ok(settings::settings_from_entries(
            settings::valid_stored_entries(rows, None),
        ))
    }

    /// 全体設定を更新する。指定した項目だけを書き換え、更新後の設定を返す。
    /// 選択肢にない値は保存せず `InvalidData` にする。
    pub fn update_settings(&self, patch: &SettingsPatch) -> Result<AppSettings, StoreError> {
        let current = self.get_settings()?;
        let next = current.applied(patch);
        next.validate().map_err(StoreError::InvalidData)?;
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in settings::to_map(patch) {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(next)
    }

    /// プロジェクト別の上書きだけを取得する（上書きしていない項目は None）。
    pub fn project_overrides(&self, project_id: &str) -> Result<SettingsPatch, StoreError> {
        let rows = self.read_entries(
            "SELECT key, value FROM project_settings WHERE project_id = ?",
            &[project_id],
        )?;
        Ok(settings::patch_from_entries(
            settings::valid_stored_entries(rows, Some(PROJECT_OVERRIDABLE_KEYS)),
        ))
    }

    /// プロジェクト別の上書きを更新する。上書きできない項目が含まれていれば拒否する。
    /// 更新後の上書き内容を返す。
    pub fn update_project_settings(
        &self,
        project_id: &str,
        patch: &SettingsPatch,
    ) -> Result<SettingsPatch, StoreError> {
        self.get_project(project_id)?;
        if let Some(key) = patch
            .keys()
            .into_iter()
            .find(|k| !PROJECT_OVERRIDABLE_KEYS.contains(&k.as_str()))
        {
            return Err(StoreError::InvalidData(format!(
                "{key} はプロジェクトごとには設定できません"
            )));
        }
        let effective = self
            .get_settings()?
            .applied(&self.project_overrides(project_id)?)
            .applied(patch);
        effective.validate().map_err(StoreError::InvalidData)?;
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in settings::to_map(patch) {
            tx.execute(
                "INSERT INTO project_settings (project_id, key, value) VALUES (?, ?, ?)
                 ON CONFLICT(project_id, key) DO UPDATE SET value = excluded.value",
                params![project_id, key, value.to_string()],
            )?;
        }
        tx.commit()?;
        self.project_overrides(project_id)
    }

    /// 実際に使う設定（全体設定にプロジェクト別の上書きを重ねたもの）。
    pub fn effective_settings(&self, project_id: &str) -> Result<AppSettings, StoreError> {
        Ok(self
            .get_settings()?
            .applied(&self.project_overrides(project_id)?))
    }

    /// 操作ジャーナルへ 1 件追記する。文字列は `redact` を通して認証情報を伏せる。
    pub fn record_journal(&self, entry: &NewJournalEntry) -> Result<i64, StoreError> {
        let clean = |s: &Option<String>| s.as_deref().map(redact);
        self.conn.execute(
            "INSERT INTO journal
                (project_id, operation, triggered_by, started_at, finished_at, outcome,
                 detail, snapshot_ref, backup_ref, target)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                entry.project_id,
                redact(&entry.operation),
                entry.trigger.as_str(),
                entry.started_at,
                entry.finished_at,
                entry.outcome.as_str(),
                clean(&entry.detail),
                clean(&entry.snapshot_ref),
                clean(&entry.backup_ref),
                clean(&entry.target),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// プロジェクトのジャーナルを新しい順に取得する。
    pub fn list_journal(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, operation, triggered_by, started_at, finished_at, outcome,
                    detail, snapshot_ref, backup_ref, target
             FROM journal
             WHERE project_id = ?
             ORDER BY started_at DESC, id DESC
             LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![project_id, limit], |row| {
                Ok(JournalEntry {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    operation: row.get(2)?,
                    trigger: JournalTrigger::from_db(&row.get::<_, String>(3)?),
                    started_at: row.get(4)?,
                    finished_at: row.get(5)?,
                    outcome: JournalOutcome::from_db(&row.get::<_, String>(6)?),
                    detail: row.get(7)?,
                    snapshot_ref: row.get(8)?,
                    backup_ref: row.get(9)?,
                    target: row.get(10)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// 操作の開始を記録する（結果は `finish_journal` で更新する）。行の ID を返す。
    pub fn begin_journal(&self, entry: &RunningJournal) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO journal
                (project_id, operation, triggered_by, started_at, finished_at, outcome, target)
             VALUES (?, ?, ?, ?, NULL, 'running', ?)",
            params![
                entry.project_id,
                redact(&entry.operation),
                entry.trigger.as_str(),
                entry.started_at,
                entry.target.as_deref().map(redact),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 「実行中」の行を完了の内容に更新する。更新したら true。
    /// 実行中でない行（すでに完了・中断として扱われた行）は変更しない。
    pub fn finish_journal(&self, id: i64, finish: &JournalFinish) -> Result<bool, StoreError> {
        let clean = |s: &Option<String>| s.as_deref().map(redact);
        let outcome = match finish.outcome {
            JournalOutcome::Success => JournalOutcome::Success,
            // 完了として書き込めるのは成功と失敗だけ
            _ => JournalOutcome::Failure,
        };
        let changed = self.conn.execute(
            "UPDATE journal
             SET finished_at = ?, outcome = ?, detail = ?, snapshot_ref = ?, backup_ref = ?
             WHERE id = ? AND outcome = 'running'",
            params![
                finish.finished_at,
                outcome.as_str(),
                clean(&finish.detail),
                clean(&finish.snapshot_ref),
                clean(&finish.backup_ref),
                id,
            ],
        )?;
        Ok(changed > 0)
    }

    /// 「実行中」の行を削除する（自動実行で何も起きなかった操作を記録に残さないため）。
    pub fn discard_journal(&self, id: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "DELETE FROM journal WHERE id = ? AND outcome = 'running'",
            params![id],
        )?;
        Ok(changed > 0)
    }

    /// 起動時に呼ぶ。「実行中」のまま残っている行（前回の終了で途中で止まった操作）を
    /// 「中断」に変え、その一覧を返す。操作が実行されていない起動直後にだけ呼ぶこと。
    pub fn recover_interrupted(&self, now: &str) -> Result<Vec<JournalEntry>, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let ids: Vec<i64> = {
            let mut stmt =
                tx.prepare("SELECT id FROM journal WHERE outcome = 'running' ORDER BY id")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<SqliteResult<Vec<_>>>()?;
            rows
        };
        for id in &ids {
            tx.execute(
                "UPDATE journal SET outcome = 'interrupted', finished_at = ?, detail = 'interrupted'
                 WHERE id = ?",
                params![now, id],
            )?;
        }
        tx.commit()?;
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.journal_rows("WHERE id = ?", params![id])?);
        }
        Ok(out)
    }

    /// プロジェクトの中断された操作のうち、最も新しいもの（要対応事項。設計書 9.2）。
    pub fn interrupted_operation(
        &self,
        project_id: &str,
    ) -> Result<Option<JournalEntry>, StoreError> {
        Ok(self
            .journal_rows(
                "WHERE project_id = ? AND outcome = 'interrupted' ORDER BY id DESC LIMIT 1",
                params![project_id],
            )?
            .into_iter()
            .next())
    }

    /// 中断された操作への対応が済んだものとして、「中断」を「失敗（interrupted）」に変える。
    /// 変えた件数を返す。
    pub fn resolve_interrupted(&self, project_id: &str) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE journal SET outcome = 'failure'
             WHERE project_id = ? AND outcome = 'interrupted'",
            params![project_id],
        )?)
    }

    /// 保持期間を過ぎた完了済みの行を削除する。削除した件数を返す。
    ///
    /// - `cutoff` より前（RFC3339。`journal_cutoff` で作る）に始まった「成功」「失敗」の行だけが対象。
    ///   「実行中」「中断」の行と、日時を読めない行は消さない
    /// - `project_id` が `Some` ならそのプロジェクトの行、`None` なら登録されていないプロジェクトの行
    /// - 復元点（refs）の削除は行わない
    pub fn purge_journal(
        &self,
        project_id: Option<&str>,
        cutoff: &str,
    ) -> Result<usize, StoreError> {
        const OLD: &str = "outcome IN ('success', 'failure')
             AND julianday(started_at) IS NOT NULL
             AND julianday(started_at) < julianday(?)";
        let changed = match project_id {
            Some(id) => self.conn.execute(
                &format!("DELETE FROM journal WHERE project_id = ? AND {OLD}"),
                params![id, cutoff],
            )?,
            None => self.conn.execute(
                &format!(
                    "DELETE FROM journal WHERE project_id NOT IN (SELECT id FROM projects) AND {OLD}"
                ),
                params![cutoff],
            )?,
        };
        Ok(changed)
    }

    /// 指定した操作が最後に成功した時刻（RFC3339）。一度も無ければ None。
    /// 「最後の自動保存」（操作名 `auto-snapshot`）の表示に使う。
    pub fn last_success_at(
        &self,
        project_id: &str,
        operation: &str,
    ) -> Result<Option<String>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT finished_at FROM journal
             WHERE project_id = ? AND operation = ? AND outcome = 'success'
               AND finished_at IS NOT NULL
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![project_id, operation])?;
        match rows.next()? {
            Some(row) => Ok(row.get::<_, Option<String>>(0)?),
            None => Ok(None),
        }
    }

    /// 利用者の手動の操作が作った復元点の ref（間引きの保護対象。設計書 6.2）。
    pub fn manual_restore_refs(&self, project_id: &str) -> Result<Vec<String>, StoreError> {
        let mut refs = Vec::new();
        for entry in self.journal_rows(
            "WHERE project_id = ? AND triggered_by = 'manual'",
            params![project_id],
        )? {
            refs.extend(entry.snapshot_ref);
            refs.extend(entry.backup_ref);
        }
        refs.sort();
        refs.dedup();
        Ok(refs)
    }

    /// 間引きで削除された復元点を、ジャーナルの参照から外す（存在しない ref を指さないため）。
    /// 外した件数を返す。
    pub fn forget_restore_refs(
        &self,
        project_id: &str,
        refs: &[String],
    ) -> Result<usize, StoreError> {
        let mut changed = 0;
        for r in refs {
            changed += self.conn.execute(
                "UPDATE journal SET snapshot_ref = NULL WHERE project_id = ? AND snapshot_ref = ?",
                params![project_id, r],
            )?;
            changed += self.conn.execute(
                "UPDATE journal SET backup_ref = NULL WHERE project_id = ? AND backup_ref = ?",
                params![project_id, r],
            )?;
        }
        Ok(changed)
    }

    fn journal_rows(
        &self,
        clause: &str,
        args: impl rusqlite::Params,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        let sql = format!(
            "SELECT id, project_id, operation, triggered_by, started_at, finished_at, outcome,
                    detail, snapshot_ref, backup_ref, target
             FROM journal {clause}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(args, |row| {
                Ok(JournalEntry {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    operation: row.get(2)?,
                    trigger: JournalTrigger::from_db(&row.get::<_, String>(3)?),
                    started_at: row.get(4)?,
                    finished_at: row.get(5)?,
                    outcome: JournalOutcome::from_db(&row.get::<_, String>(6)?),
                    detail: row.get(7)?,
                    snapshot_ref: row.get(8)?,
                    backup_ref: row.get(9)?,
                    target: row.get(10)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// 登録パスだけを更新する（フォルダの付け替え用）。他の項目は変えない。
    pub fn update_project_path(&self, id: &str, path: &Path) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET path = ? WHERE id = ?",
            params![path.to_string_lossy().to_string(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::ProjectNotFound(id.to_string()));
        }
        Ok(())
    }

    /// 最初の保存（初期 commit）の OID を記録する。付け替え時の照合用。
    /// 同じ値を何度書いても変わらない。別の値が記録済みなら上書きしない（履歴は変わらないため）。
    pub fn set_initial_commit(&self, id: &str, oid: &str) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET initial_commit = ? WHERE id = ? AND initial_commit IS NULL",
            params![oid, id],
        )?;
        if changed == 0 {
            // 記録済み、またはプロジェクトが無い。後者だけをエラーにする
            self.get_project(id)?;
        }
        Ok(())
    }

    /// 最終表示日時を更新。
    pub fn touch_project(&self, id: &str) -> Result<(), StoreError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET last_viewed_at = ? WHERE id = ?",
            params![now, id],
        )?;
        Ok(())
    }
}

/// プロジェクト単位の直列実行ロック。状態変更操作の並行実行を防ぐ。
/// Tauri 非依存で、単体テスト可能。
pub struct ProjectLocks {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ProjectLocks {
    /// 新規作成
    pub fn new() -> Self {
        ProjectLocks {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// プロジェクト ID のロックを取得してクロージャを実行。
    /// 同一 ID への操作は直列実行（クリティカルセクション内で f を実行）。
    pub fn run<T, F>(&self, project_id: &str, f: F) -> Result<T, String>
    where
        F: FnOnce() -> T,
    {
        let mut locks = self.locks.lock().map_err(|e| e.to_string())?;
        let lock_arc = locks
            .entry(project_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        // locks の Mutex を drop（ロック取得の前に map からリリース）
        drop(locks);

        // lock_arc のロックを取得して f を実行
        let _guard = lock_arc.lock().map_err(|e| e.to_string())?;
        Ok(f())
    }
}

impl Default for ProjectLocks {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ProjectLocks {
    fn clone(&self) -> Self {
        ProjectLocks {
            locks: Mutex::new(
                self.locks
                    .lock()
                    .expect("lock poison")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn setup_test_db() -> (Store, TempDir) {
        let tmpdir = TempDir::new().expect("failed to create temp dir");
        let db_path = tmpdir.path().join("test.db");
        let store = Store::open(&db_path).expect("failed to open store");
        (store, tmpdir)
    }

    #[test]
    fn test_update_project_path_changes_only_path() {
        let (store, _tmpdir) = setup_test_db();
        store
            .add_project(
                "proj-1",
                "My Project",
                Path::new("/old/path"),
                Some("https://github.com/user/repo"),
                "myuser",
                "main",
            )
            .expect("failed to add project");
        store
            .update_project_path("proj-1", Path::new("/new/path"))
            .expect("failed to update path");
        let proj = store.get_project("proj-1").expect("failed to get project");
        assert_eq!(proj.path, Path::new("/new/path"));
        assert_eq!(proj.display_name, "My Project");
        assert_eq!(
            proj.remote_url.as_deref(),
            Some("https://github.com/user/repo")
        );
        assert!(store
            .update_project_path("missing", Path::new("/x"))
            .is_err());
    }

    #[test]
    fn test_set_initial_commit_records_once() {
        let (store, _tmpdir) = setup_test_db();
        store
            .add_project("p", "P", Path::new("/p"), None, "me", "main")
            .expect("add");
        assert_eq!(store.get_project("p").expect("get").initial_commit, None);

        store.set_initial_commit("p", "aaa111").expect("set");
        assert_eq!(
            store
                .get_project("p")
                .expect("get")
                .initial_commit
                .as_deref(),
            Some("aaa111")
        );
        // 履歴の最初の保存は変わらないので、記録済みなら上書きしない
        store.set_initial_commit("p", "bbb222").expect("set again");
        assert_eq!(
            store
                .get_project("p")
                .expect("get")
                .initial_commit
                .as_deref(),
            Some("aaa111")
        );
        // 付け替え（パス変更）では消えない
        store
            .update_project_path("p", Path::new("/q"))
            .expect("path");
        assert_eq!(
            store
                .get_project("p")
                .expect("get")
                .initial_commit
                .as_deref(),
            Some("aaa111")
        );
        assert!(store.set_initial_commit("missing", "x").is_err());
    }

    #[test]
    fn test_add_and_get_project() {
        let (store, _tmpdir) = setup_test_db();

        store
            .add_project(
                "proj-1",
                "My Project",
                Path::new("/path/to/project"),
                Some("https://github.com/user/repo"),
                "myuser",
                "main",
            )
            .expect("failed to add project");

        let proj = store.get_project("proj-1").expect("failed to get project");
        assert_eq!(proj.id, "proj-1");
        assert_eq!(proj.display_name, "My Project");
        assert_eq!(proj.owner, "myuser");
    }

    #[test]
    fn test_list_projects() {
        let (store, _tmpdir) = setup_test_db();

        store
            .add_project(
                "proj-1",
                "Project 1",
                Path::new("/path/1"),
                None,
                "user",
                "main",
            )
            .expect("failed to add project");

        store
            .add_project(
                "proj-2",
                "Project 2",
                Path::new("/path/2"),
                None,
                "user",
                "main",
            )
            .expect("failed to add project");

        let projects = store.list_projects().expect("failed to list projects");
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_remove_project() {
        let (store, _tmpdir) = setup_test_db();

        store
            .add_project(
                "proj-1",
                "Project",
                Path::new("/path"),
                None,
                "user",
                "main",
            )
            .expect("failed to add project");

        store.remove_project("proj-1").expect("failed to remove");

        let result = store.get_project("proj-1");
        assert!(matches!(result, Err(StoreError::ProjectNotFound(_))));
    }

    #[test]
    fn test_touch_project() {
        let (store, _tmpdir) = setup_test_db();

        store
            .add_project(
                "proj-1",
                "Project",
                Path::new("/path"),
                None,
                "user",
                "main",
            )
            .expect("failed to add");

        let proj1 = store.get_project("proj-1").expect("failed to get");
        let time1 = proj1.last_viewed_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        store.touch_project("proj-1").expect("failed to touch");

        let proj2 = store.get_project("proj-1").expect("failed to get");
        let time2 = proj2.last_viewed_at;

        assert!(time2 > time1);
    }

    /// 同一 ID での並行実行が直列化されることを検証。
    /// AtomicUsize で同時実行数の最大値を測定。
    #[test]
    fn test_project_locks_same_id_serializes() {
        use std::sync::Arc;

        let locks = Arc::new(ProjectLocks::new());
        let concurrent_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        for _ in 0..4 {
            let locks_clone = locks.clone();
            let count_clone = concurrent_count.clone();
            let max_clone = max_concurrent.clone();

            let handle = std::thread::spawn(move || {
                locks_clone
                    .run("proj-1", || {
                        // クリティカルセクション開始
                        let now = count_clone.fetch_add(1, Ordering::SeqCst);
                        let new_max = now + 1;

                        // max_concurrent を更新（最大同時実行数を記録）
                        let mut old_max = max_clone.load(Ordering::SeqCst);
                        while new_max > old_max {
                            match max_clone.compare_exchange(
                                old_max,
                                new_max,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            ) {
                                Ok(_) => break,
                                Err(actual) => old_max = actual,
                            }
                        }

                        std::thread::sleep(std::time::Duration::from_millis(10));

                        // クリティカルセクション終了
                        count_clone.fetch_sub(1, Ordering::SeqCst);
                    })
                    .expect("run failed")
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread join failed");
        }

        let max = max_concurrent.load(Ordering::SeqCst);
        assert_eq!(
            max, 1,
            "同一 ID でのロック: 最大同時実行数は 1 であるべき（実際: {}）",
            max
        );
    }

    /// 異なる ID での並行実行が可能であることを検証。
    /// 時間の計測ではなく、双方がクリティカルセクションに同時にいることを観測する（CI の負荷で揺れない）。
    #[test]
    fn test_project_locks_different_ids_parallel() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let locks = Arc::new(ProjectLocks::new());
        let inside = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let met = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let spawn = |id: &'static str| {
            let locks = locks.clone();
            let inside = inside.clone();
            let overlapped = overlapped.clone();
            let met = met.clone();
            std::thread::spawn(move || {
                locks
                    .run(id, || {
                        inside.fetch_add(1, Ordering::SeqCst);
                        // 相手が入ってくるまで最大 5 秒待つ。直列化されていれば相手は入れない
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while !met.load(Ordering::SeqCst) && Instant::now() < deadline {
                            if inside.load(Ordering::SeqCst) == 2 {
                                met.store(true, Ordering::SeqCst);
                            }
                            std::thread::yield_now();
                        }
                        if met.load(Ordering::SeqCst) {
                            overlapped.fetch_add(1, Ordering::SeqCst);
                        }
                        inside.fetch_sub(1, Ordering::SeqCst);
                    })
                    .expect("run failed")
            })
        };

        let h1 = spawn("proj-1");
        let h2 = spawn("proj-2");
        h1.join().expect("thread join failed");
        h2.join().expect("thread join failed");

        assert!(
            overlapped.load(Ordering::SeqCst) >= 1,
            "異なる ID のロックは並行実行できるはず"
        );
    }
}
