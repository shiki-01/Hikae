// 登録内容に合わせて、監視と実行計画を整理する判断（設計書 8.3、4.6「一覧から外す」）。
//
// プロジェクトの登録を外す・フォルダを付け替える・フォルダが見つからなくなると、そのプロジェクトの
// ファイル監視と、取り込み・アップロード・自動保存の実行計画はすぐに止めなければならない。
// どれを止めるかを tauri 非依存の純関数にして、`app` のスケジューラと監視の管理が同じ判断を使う。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 止めるべき監視のプロジェクト ID を返す。登録に無い、またはフォルダの場所が変わったもの。
/// 入力は、いま動いている監視の（ID, フォルダ）と、これから動かすべき（ID, フォルダ）。
pub fn stale_watches<'a>(
    running: impl IntoIterator<Item = (&'a str, &'a Path)>,
    targets: &[(String, PathBuf)],
) -> Vec<String> {
    running
        .into_iter()
        .filter(|(id, path)| {
            !targets
                .iter()
                .any(|(tid, tpath)| tid == id && tpath.as_path() == *path)
        })
        .map(|(id, _)| id.to_string())
        .collect()
}

/// 登録の無いプロジェクトの項目（実行計画など）を捨てる。捨てた ID を返す。
pub fn retain_registered<V>(map: &mut HashMap<String, V>, wanted: &HashSet<&str>) -> Vec<String> {
    let removed: Vec<String> = map
        .keys()
        .filter(|id| !wanted.contains(id.as_str()))
        .cloned()
        .collect();
    for id in &removed {
        map.remove(id);
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SnapshotPlanner, SnapshotPolicy, SyncPlanner, SyncPolicy};

    fn target(id: &str, path: &str) -> (String, PathBuf) {
        (id.to_string(), PathBuf::from(path))
    }

    #[test]
    fn removed_and_moved_projects_are_stale() {
        let running = [
            ("keep", Path::new("/a")),
            ("removed", Path::new("/b")),
            ("moved", Path::new("/c")),
        ];
        let targets = vec![target("keep", "/a"), target("moved", "/c2")];
        let mut stale = stale_watches(running, &targets);
        stale.sort();
        assert_eq!(stale, vec!["moved".to_string(), "removed".to_string()]);
    }

    #[test]
    fn nothing_is_stale_when_the_registration_is_unchanged() {
        let running = [("a", Path::new("/a"))];
        assert!(stale_watches(running, &[target("a", "/a")]).is_empty());
        // 登録がすべて外されたら、すべて止める
        assert_eq!(stale_watches(running, &[]), vec!["a".to_string()]);
    }

    #[test]
    fn unregistered_plans_are_dropped_and_no_longer_wake_the_scheduler() {
        let mut snapshots: HashMap<String, SnapshotPlanner> = HashMap::new();
        let mut syncs: HashMap<String, SyncPlanner> = HashMap::new();
        for id in ["keep", "gone"] {
            let mut planner = SnapshotPlanner::new(SnapshotPolicy::default());
            planner.note_change(10);
            snapshots.insert(id.to_string(), planner);
            syncs.insert(
                id.to_string(),
                SyncPlanner::new(SyncPolicy::from_config(Some(true), Some(15), Some(true)), 0),
            );
        }
        let next_snapshot =
            |m: &HashMap<String, SnapshotPlanner>| m.values().filter_map(|p| p.next_at()).min();
        assert!(next_snapshot(&snapshots).is_some());

        let wanted: HashSet<&str> = ["keep"].into_iter().collect();
        assert_eq!(
            retain_registered(&mut snapshots, &wanted),
            vec!["gone".to_string()]
        );
        assert_eq!(
            retain_registered(&mut syncs, &wanted),
            vec!["gone".to_string()]
        );
        assert!(!snapshots.contains_key("gone"));
        assert!(!syncs.contains_key("gone"));

        // 全員を外すと、待つべき予定が無くなる（自動保存も、取り込み・アップロードも動かない）
        let none: HashSet<&str> = HashSet::new();
        retain_registered(&mut snapshots, &none);
        retain_registered(&mut syncs, &none);
        assert_eq!(next_snapshot(&snapshots), None);
        assert!(syncs
            .values()
            .filter_map(|p| p.next_wake())
            .next()
            .is_none());
    }
}
