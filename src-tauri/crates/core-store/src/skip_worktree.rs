// 「今後の変更だけ無視する」（skip-worktree、設計書 4.8）の記録。
//
// - 記録は PC ごと（この PC の SQLite）。git 側の印（インデックスの skip-worktree ビット）は
//   リポジトリの外に出ない。他の PC へは届かないため、記録は「この PC で無視を指定したもの」を表す
// - 記録と git 側の実際の状態がずれうる（復元・再取得などで印が外れる）。どちらが正かは
//   core-ops が git に問い合わせて決める。ここでは記録を保持するだけ
// - `dismissed` は「プロジェクトを開いたとき」の再適用の提案で「今はしない」を選んだ印。
//   無視を指定し直すと 0 に戻す

use crate::{Store, StoreError};
use rusqlite::params;

/// skip-worktree の記録 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipWorktreeRecord {
    /// プロジェクトからの相対パス（`/` 区切り）
    pub path: String,
    /// 無視を指定した時刻（RFC 3339）
    pub set_at: String,
    /// 再適用の提案で「今はしない」を選んだ（再通知しない）
    pub dismissed: bool,
}

impl Store {
    /// プロジェクトの記録を、パスの順で返す。
    pub fn list_skip_worktree(
        &self,
        project_id: &str,
    ) -> Result<Vec<SkipWorktreeRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, set_at, dismissed FROM skip_worktree WHERE project_id = ? ORDER BY path",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(SkipWorktreeRecord {
                    path: row.get(0)?,
                    set_at: row.get(1)?,
                    dismissed: row.get::<_, i64>(2)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 記録を追加する。同じパスが既にあれば、時刻を更新し、提案の印（dismissed）を 0 に戻す。
    pub fn add_skip_worktree(
        &self,
        project_id: &str,
        paths: &[String],
        set_at: &str,
    ) -> Result<(), StoreError> {
        for path in paths {
            self.conn.execute(
                "INSERT INTO skip_worktree (project_id, path, set_at, dismissed)
                 VALUES (?1, ?2, ?3, 0)
                 ON CONFLICT (project_id, path)
                 DO UPDATE SET set_at = excluded.set_at, dismissed = 0",
                params![project_id, path, set_at],
            )?;
        }
        Ok(())
    }

    /// 記録を削除する。無いパスは何もしない。削除した件数を返す。
    pub fn remove_skip_worktree(
        &self,
        project_id: &str,
        paths: &[String],
    ) -> Result<usize, StoreError> {
        let mut removed = 0;
        for path in paths {
            removed += self.conn.execute(
                "DELETE FROM skip_worktree WHERE project_id = ?1 AND path = ?2",
                params![project_id, path],
            )?;
        }
        Ok(removed)
    }

    /// 記録の `dismissed` を設定する。記録が無いパスは何もしない。
    pub fn set_skip_worktree_dismissed(
        &self,
        project_id: &str,
        paths: &[String],
        dismissed: bool,
    ) -> Result<(), StoreError> {
        for path in paths {
            self.conn.execute(
                "UPDATE skip_worktree SET dismissed = ?3 WHERE project_id = ?1 AND path = ?2",
                params![project_id, path, i64::from(dismissed)],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup() -> (Store, TempDir) {
        let tmp = TempDir::new().expect("temp dir");
        let store = Store::open(&tmp.path().join("test.db")).expect("open");
        store
            .add_project("p1", "P", Path::new("/p1"), None, "me", "main")
            .expect("add project");
        store
            .add_project("p2", "Q", Path::new("/p2"), None, "me", "main")
            .expect("add project");
        (store, tmp)
    }

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_list_and_remove_are_scoped_to_the_project() {
        let (store, _tmp) = setup();
        store
            .add_skip_worktree("p1", &paths(&["b.psd", "a.psd"]), "2026-10-10T00:00:00Z")
            .expect("add");
        store
            .add_skip_worktree("p2", &paths(&["c.psd"]), "2026-10-10T00:00:00Z")
            .expect("add other");

        let listed = store.list_skip_worktree("p1").expect("list");
        let names: Vec<&str> = listed.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(names, vec!["a.psd", "b.psd"]);
        assert!(listed.iter().all(|r| !r.dismissed));
        assert_eq!(store.list_skip_worktree("p2").expect("list").len(), 1);

        assert_eq!(
            store
                .remove_skip_worktree("p1", &paths(&["a.psd", "missing"]))
                .expect("remove"),
            1
        );
        let after = store.list_skip_worktree("p1").expect("list");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].path, "b.psd");
    }

    #[test]
    fn dismissed_flag_is_set_and_reset_by_re_adding() {
        let (store, _tmp) = setup();
        store
            .add_skip_worktree("p1", &paths(&["a.psd"]), "2026-10-10T00:00:00Z")
            .expect("add");
        store
            .set_skip_worktree_dismissed("p1", &paths(&["a.psd"]), true)
            .expect("dismiss");
        assert!(store.list_skip_worktree("p1").expect("list")[0].dismissed);

        // 同じパスを指定し直すと、時刻が更新され、提案の印は戻る
        store
            .add_skip_worktree("p1", &paths(&["a.psd"]), "2026-10-11T00:00:00Z")
            .expect("re-add");
        let rec = &store.list_skip_worktree("p1").expect("list")[0];
        assert!(!rec.dismissed);
        assert_eq!(rec.set_at, "2026-10-11T00:00:00Z");
    }

    #[test]
    fn dismissing_a_missing_record_does_nothing() {
        let (store, _tmp) = setup();
        store
            .set_skip_worktree_dismissed("p1", &paths(&["none.psd"]), true)
            .expect("no error");
        assert!(store.list_skip_worktree("p1").expect("list").is_empty());
    }
}
