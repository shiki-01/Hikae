// ローカル LLM の永続データ（設計書 9.1、10.4、10.5）。
//
// - `llm_models`: 導入済みモデルの記録（ID、段階、保存先の相対パス、SHA-256、サイズ、導入日時）。
//   導入の有無の判定はファイルの有無で行い、この表は記録として残す
// - `message_cache`: 生成済みの保存メモ。キーは呼び出し側が作る入力ハッシュ（`core-llm` の `cache_key`）。
//   同じ入力は LLM を呼ばずに返す。保持は `MESSAGE_CACHE_LIMIT` 件まで（古い順に削除）
// - 値はそのまま保存・取得する。モデルの内容・ファイルの内容・パスは、ここでは検査もログもしない

use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

/// 生成済みメモのキャッシュの保持件数（設計書 10.4）
pub const MESSAGE_CACHE_LIMIT: usize = 500;

/// 導入済みモデルの記録
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmModelRecord {
    /// モデルの ID（カタログの ID と同じ）
    pub id: String,
    /// 段階（設計書 10.2）
    pub stage: u8,
    /// `models_dir` からの相対パス（例: `gemma-4-e2b-it-q4_k_m/gemma-4-E2B-it-Q4_K_M.gguf`）
    pub file_path: String,
    /// SHA-256（小文字の 16 進）。検証を経ていない記録は None
    pub sha256: Option<String>,
    /// ファイルのサイズ（バイト）
    pub size_bytes: Option<i64>,
    /// 導入日時（RFC3339）
    pub installed_at: String,
}

impl Store {
    /// 導入済みモデルの記録を追加・更新する（同じ ID は上書き）。
    pub fn upsert_llm_model(&self, record: &LlmModelRecord) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_models (id, stage, file_path, sha256, size_bytes, installed_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                stage = excluded.stage,
                file_path = excluded.file_path,
                sha256 = excluded.sha256,
                size_bytes = excluded.size_bytes,
                installed_at = excluded.installed_at",
            params![
                record.id,
                i64::from(record.stage),
                record.file_path,
                record.sha256,
                record.size_bytes,
                record.installed_at,
            ],
        )?;
        Ok(())
    }

    /// 導入済みモデルの記録を 1 件取得する。
    pub fn get_llm_model(&self, id: &str) -> Result<Option<LlmModelRecord>, StoreError> {
        let record = self
            .conn
            .query_row(
                "SELECT id, stage, file_path, sha256, size_bytes, installed_at
                 FROM llm_models WHERE id = ?",
                params![id],
                row_to_record,
            )
            .optional()?;
        Ok(record)
    }

    /// 導入済みモデルの記録を、ID の昇順ですべて取得する。
    pub fn list_llm_models(&self) -> Result<Vec<LlmModelRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, stage, file_path, sha256, size_bytes, installed_at
             FROM llm_models ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], row_to_record)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 導入済みモデルの記録を削除する（ファイルの削除は呼び出し側）。
    pub fn remove_llm_model(&self, id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM llm_models WHERE id = ?", params![id])?;
        Ok(())
    }

    /// 生成済みメモを、入力ハッシュで取得する。
    pub fn get_cached_message(&self, input_hash: &str) -> Result<Option<String>, StoreError> {
        let message = self
            .conn
            .query_row(
                "SELECT message FROM message_cache WHERE input_hash = ?",
                params![input_hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(message)
    }

    /// 生成済みメモを保存する。同じ入力ハッシュは上書きし、件数が `MESSAGE_CACHE_LIMIT` を
    /// 超えたら作成の古いものから削除する（同じ時刻の場合は挿入の古いものから）。
    pub fn put_cached_message(
        &self,
        input_hash: &str,
        model_id: &str,
        message: &str,
        created_at: &str,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        // 上書きは削除して入れ直すため、挿入順（rowid）が新しくなる
        tx.execute(
            "DELETE FROM message_cache WHERE input_hash = ?",
            params![input_hash],
        )?;
        tx.execute(
            "INSERT INTO message_cache (input_hash, model_id, message, created_at)
             VALUES (?, ?, ?, ?)",
            params![input_hash, model_id, message, created_at],
        )?;
        tx.execute(
            "DELETE FROM message_cache WHERE rowid NOT IN (
                SELECT rowid FROM message_cache ORDER BY created_at DESC, rowid DESC LIMIT ?
             )",
            params![i64::try_from(MESSAGE_CACHE_LIMIT).unwrap_or(i64::MAX)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 生成済みメモの件数（テストと診断用）
    pub fn count_cached_messages(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM message_cache", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LlmModelRecord> {
    let stage: i64 = row.get(1)?;
    Ok(LlmModelRecord {
        id: row.get(0)?,
        stage: u8::try_from(stage).unwrap_or(0),
        file_path: row.get(2)?,
        sha256: row.get(3)?,
        size_bytes: row.get(4)?,
        installed_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open() -> (Store, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let store = Store::open(&dir.path().join("test.db")).expect("open");
        (store, dir)
    }

    fn record(id: &str, stage: u8) -> LlmModelRecord {
        LlmModelRecord {
            id: id.to_string(),
            stage,
            file_path: format!("{id}/model.gguf"),
            sha256: Some("a".repeat(64)),
            size_bytes: Some(1024),
            installed_at: "2026-10-10T00:00:00+00:00".to_string(),
        }
    }

    #[test]
    fn llm_models_round_trip_upsert_and_remove() {
        let (store, _dir) = open();
        assert!(store.list_llm_models().expect("list").is_empty());

        store.upsert_llm_model(&record("m-b", 2)).expect("insert");
        store.upsert_llm_model(&record("m-a", 2)).expect("insert");
        let ids: Vec<String> = store
            .list_llm_models()
            .expect("list")
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["m-a", "m-b"]);

        // 同じ ID は上書きされる
        let mut changed = record("m-a", 2);
        changed.size_bytes = None;
        changed.sha256 = None;
        store.upsert_llm_model(&changed).expect("upsert");
        assert_eq!(
            store.get_llm_model("m-a").expect("get"),
            Some(changed.clone())
        );
        assert_eq!(store.list_llm_models().expect("list").len(), 2);

        store.remove_llm_model("m-a").expect("remove");
        assert_eq!(store.get_llm_model("m-a").expect("get"), None);
        assert_eq!(store.list_llm_models().expect("list").len(), 1);
    }

    #[test]
    fn message_cache_returns_the_stored_message_for_the_same_key() {
        let (store, _dir) = open();
        assert_eq!(store.get_cached_message("k1").expect("get"), None);
        store
            .put_cached_message("k1", "m", "first", "2026-10-10T00:00:00+00:00")
            .expect("put");
        assert_eq!(
            store.get_cached_message("k1").expect("get"),
            Some("first".to_string())
        );
        // 同じキーは上書き（件数は増えない）
        store
            .put_cached_message("k1", "m", "second", "2026-10-10T00:00:01+00:00")
            .expect("put again");
        assert_eq!(
            store.get_cached_message("k1").expect("get"),
            Some("second".to_string())
        );
        assert_eq!(store.count_cached_messages().expect("count"), 1);
    }

    #[test]
    fn message_cache_keeps_at_most_the_limit_and_drops_the_oldest() {
        let (store, _dir) = open();
        let total = MESSAGE_CACHE_LIMIT + 3;
        for i in 0..total {
            // 作成時刻は 1 件ずつ新しくする（秒で区別できる形式）
            let created = format!("2026-10-10T00:{:02}:{:02}+00:00", i / 60, i % 60);
            store
                .put_cached_message(&format!("k{i}"), "m", &format!("v{i}"), &created)
                .expect("put");
        }
        assert_eq!(
            store.count_cached_messages().expect("count"),
            MESSAGE_CACHE_LIMIT
        );
        // 古い 3 件が消え、新しいものは残る
        for i in 0..3 {
            assert_eq!(
                store.get_cached_message(&format!("k{i}")).expect("get"),
                None,
                "k{i} should be pruned"
            );
        }
        assert_eq!(
            store
                .get_cached_message(&format!("k{}", total - 1))
                .expect("get"),
            Some(format!("v{}", total - 1))
        );
    }

    #[test]
    fn message_cache_prunes_by_creation_time_not_by_insert_order() {
        let (store, _dir) = open();
        // 先に入れた行が、作成時刻では新しい場合は残る
        store
            .put_cached_message("late", "m", "late", "2026-10-10T12:00:00+00:00")
            .expect("put");
        for i in 0..MESSAGE_CACHE_LIMIT {
            let created = format!("2026-10-10T00:{:02}:{:02}+00:00", i / 60, i % 60);
            store
                .put_cached_message(&format!("k{i}"), "m", "v", &created)
                .expect("put");
        }
        assert_eq!(
            store.get_cached_message("late").expect("get"),
            Some("late".to_string())
        );
        assert_eq!(store.get_cached_message("k0").expect("get"), None);
    }
}
