// 拡張機能の有効・無効と登録の記録（設計書 9.1、11.9）。テーブル `extensions` は v1 で作成済みのものを使う。
//
// - 行が無い拡張は有効（既定）。無効の行を持つ「追加分」は、起動時に登録しない（app の load_extensions）
// - 標準の拡張（resources/extensions）は、この記録に関係なく登録する（無効にできない）
// - `version` は記録したときの版（情報用）。有効・無効の判定には使わない
// - `permissions` 列は既定の `[]` のまま（権限は受け付けない。11.2）

use crate::{Store, StoreError};
use rusqlite::params;

/// 拡張の記録 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionState {
    /// 拡張の ID（マニフェストの `id`）
    pub id: String,
    /// 記録したときの版（マニフェストの `version`）
    pub version: String,
    /// 有効なら true。無効の追加分は起動時に登録しない
    pub enabled: bool,
}

impl Store {
    /// 拡張の記録を、ID の順で返す
    pub fn list_extension_states(&self) -> Result<Vec<ExtensionState>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, version, enabled FROM extensions ORDER BY id")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExtensionState {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    enabled: row.get::<_, i64>(2)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 拡張の有効・無効を記録する。行が無ければ作り、あれば版と有効の値を更新する
    pub fn set_extension_enabled(
        &self,
        id: &str,
        version: &str,
        enabled: bool,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO extensions (id, version, enabled) VALUES (?1, ?2, ?3)
             ON CONFLICT (id)
             DO UPDATE SET version = excluded.version, enabled = excluded.enabled",
            params![id, version, i64::from(enabled)],
        )?;
        Ok(())
    }

    /// 拡張の記録を消す（「一覧から外す」。フォルダの移動とは別に行う）。無ければ何もしない
    pub fn remove_extension_state(&self, id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM extensions WHERE id = ?1", params![id])?;
        Ok(())
    }
}
