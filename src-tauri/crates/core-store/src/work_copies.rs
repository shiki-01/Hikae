// 作業コピー（別の案を試す場所。設計書 4.9）の表示名と、一覧での状態（しまったかどうか）。
//
// - 内部名（Git のブランチ名。`wc-YYYYMMDD-HHMMSS`）は `branch` 列に入る。画面には出さない
// - 表示名は利用者が付けた語。内部名とは別に持つ
// - 「しまう」は削除ではなく `archived` の印（既定の一覧から隠すだけ）。削除の機能は設けない（不変条件 1）
// - Git 側の実体（ブランチ）には触れない。ここは記録だけを扱う

use crate::{Store, StoreError};
use rusqlite::params;

/// 作業コピーの記録 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCopyRecord {
    /// 内部名（Git のブランチ名）
    pub branch: String,
    /// 利用者が付けた表示名
    pub display_name: String,
    /// 作成時刻（RFC 3339）
    pub created_at: String,
    /// 一覧の既定から隠しているか（「しまう」）
    pub archived: bool,
}

impl Store {
    /// 作業コピーの記録を追加する。同じ内部名が既にあればエラー（上書きしない）。
    pub fn add_work_copy(
        &self,
        project_id: &str,
        branch: &str,
        display_name: &str,
        created_at: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO work_copies (project_id, branch, display_name, created_at, archived)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![project_id, branch, display_name, created_at],
        )?;
        Ok(())
    }

    /// プロジェクトの作業コピーの記録を、作成の順（同時刻は内部名の順）で返す。
    /// しまったものも含む（一覧の既定で隠すかどうかは呼び出し側が決める）。
    pub fn list_work_copies(&self, project_id: &str) -> Result<Vec<WorkCopyRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT branch, display_name, created_at, archived FROM work_copies
             WHERE project_id = ? ORDER BY created_at, branch",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(WorkCopyRecord {
                    branch: row.get(0)?,
                    display_name: row.get(1)?,
                    created_at: row.get(2)?,
                    archived: row.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 作業コピーを「しまう」（`archived = true`）か、しまうのをやめる（`false`）。
    /// 記録が無ければ何もせず `false` を返す。行を削除はしない。
    pub fn set_work_copy_archived(
        &self,
        project_id: &str,
        branch: &str,
        archived: bool,
    ) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE work_copies SET archived = ?3 WHERE project_id = ?1 AND branch = ?2",
            params![project_id, branch, i64::from(archived)],
        )?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_flag_round_trips_and_missing_rows_are_reported() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open(&tmp.path().join("t.db")).expect("open");
        store
            .add_work_copy("p1", "wc-20261020-120000", "案A", "2026-10-20T03:00:00Z")
            .expect("add");
        assert!(store
            .set_work_copy_archived("p1", "wc-20261020-120000", true)
            .expect("archive"));
        assert!(!store
            .set_work_copy_archived("p1", "wc-none", true)
            .expect("missing"));
        let rows = store.list_work_copies("p1").expect("list");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].archived);
    }
}
