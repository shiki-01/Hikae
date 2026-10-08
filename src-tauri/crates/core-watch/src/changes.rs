// 静止判定（デバウンス）後のファイル変更イベントを、1 回の通知（`ChangeBatch`）にまとめる純粋なロジック。
//
// notify の型には依存せず、監視の層（`watcher`）が変換した `RawEvent` を受け取る。
// 無視するパスの判定は `ignore_rules`。ファイルシステムの状態は `is_dir` を引数で受け取って参照する。

use crate::ignore_rules::{relative_to_roots, IgnoreRules};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// 1 回の通知に載せるパスの最大件数（大量の変更でも通知を軽くする）
pub const MAX_REPORTED_PATHS: usize = 100;

/// 変更の種類（notify の `EventKind` を、判断に必要な分だけに絞ったもの）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Removed,
    /// 名前の変更・移動
    Renamed,
    /// 内容やメタデータの変更
    Modified,
    /// 読み取りだけ（変更ではない）
    Access,
    /// それ以外
    Other,
}

/// デバウンス後の 1 件のイベント
#[derive(Debug, Clone)]
pub struct RawEvent {
    pub kind: ChangeKind,
    pub paths: Vec<PathBuf>,
    /// OS のイベントが溢れたなどで、取りこぼしの可能性がある（全体を調べ直す必要がある）
    pub rescan: bool,
}

/// 1 回のデバウンスで得た、保存の対象になりうる変更のまとめ
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeBatch {
    /// プロジェクトからの相対パス（重複なし・名前順）。最大 `MAX_REPORTED_PATHS` 件
    pub paths: Vec<PathBuf>,
    /// 変更されたパスの総数（`paths` に載せきれなかった分を含む）
    pub total: usize,
    /// 取りこぼしの可能性があり、全体を調べ直す必要がある
    pub rescan: bool,
}

impl ChangeBatch {
    /// 通知する必要がない（保存の対象になる変更が無い）
    pub fn is_empty(&self) -> bool {
        self.total == 0 && !self.rescan
    }

    /// `.gitignore` 自体が変わった（無視の規則を読み直す必要がある）
    pub fn touches_gitignore(&self) -> bool {
        self.rescan || self.paths.iter().any(|p| p == Path::new(".gitignore"))
    }
}

/// `.git` を含むパスか（プロジェクトの外と判定されたパスの最終確認用）
fn has_dot_git_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(name) if name == ".git"))
}

/// 保存の対象になりうる変更だけを選び、1 回の通知にまとめる。
///
/// - 読み取りだけのイベント、`.git` 配下、無視の規則に合うパスは捨てる
/// - フォルダ自体の内容変更（中のファイルの増減に伴う更新時刻の変化）は捨てる。中のファイルの
///   イベントで分かるため
/// - プロジェクトのフォルダ自体と、フォルダの外と判定されたパスは、`.git` を含まない限り残す
///   （判定できないときは通知する側に倒す）
pub fn summarize(
    rules: &IgnoreRules,
    roots: &[PathBuf],
    events: &[RawEvent],
    is_dir: impl Fn(&Path) -> bool,
) -> ChangeBatch {
    let mut batch = ChangeBatch::default();
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();

    for event in events {
        if event.rescan {
            batch.rescan = true;
        }
        if event.kind == ChangeKind::Access {
            continue;
        }
        for path in &event.paths {
            let dir = is_dir(path);
            if dir && event.kind == ChangeKind::Modified {
                continue;
            }
            match relative_to_roots(path, roots) {
                Some(rel) => {
                    if rel.as_os_str().is_empty() || rules.is_ignored(&rel, dir) {
                        continue;
                    }
                    found.insert(rel);
                }
                None => {
                    if has_dot_git_component(path) {
                        continue;
                    }
                    found.insert(path.clone());
                }
            }
        }
    }

    batch.total = found.len();
    batch.paths = found.into_iter().take(MAX_REPORTED_PATHS).collect();
    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: ChangeKind, paths: &[&str]) -> RawEvent {
        RawEvent {
            kind,
            paths: paths.iter().map(PathBuf::from).collect(),
            rescan: false,
        }
    }

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/proj")]
    }

    fn run(rules: &IgnoreRules, events: &[RawEvent]) -> ChangeBatch {
        summarize(rules, &roots(), events, |p| p.ends_with("dir"))
    }

    #[test]
    fn keeps_relative_sorted_unique_paths() {
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[
                ev(ChangeKind::Modified, &["/proj/b.txt", "/proj/a.txt"]),
                ev(ChangeKind::Modified, &["/proj/a.txt"]),
                ev(ChangeKind::Created, &["/proj/sub/c.txt"]),
            ],
        );
        assert_eq!(
            batch.paths,
            vec![
                PathBuf::from("a.txt"),
                PathBuf::from("b.txt"),
                PathBuf::from("sub/c.txt")
            ]
        );
        assert_eq!(batch.total, 3);
        assert!(!batch.is_empty());
    }

    #[test]
    fn drops_access_dot_git_ignored_and_root_events() {
        let rules = IgnoreRules::from_lines(Path::new("/proj"), &["*.log"]);
        let batch = run(
            &rules,
            &[
                ev(ChangeKind::Access, &["/proj/read.txt"]),
                ev(ChangeKind::Modified, &["/proj/.git/index"]),
                ev(ChangeKind::Created, &["/proj/~$a.docx", "/proj/x.log"]),
                ev(ChangeKind::Modified, &["/proj"]),
                // プロジェクトの外の `.git`（判定できないパス）も捨てる
                ev(ChangeKind::Modified, &["/elsewhere/.git/HEAD"]),
            ],
        );
        assert!(batch.is_empty(), "{batch:?}");
    }

    #[test]
    fn directory_modification_alone_is_not_a_change_but_create_and_remove_are() {
        let rules = IgnoreRules::builtin_only();
        assert!(run(&rules, &[ev(ChangeKind::Modified, &["/proj/dir"])]).is_empty());
        let created = run(&rules, &[ev(ChangeKind::Created, &["/proj/dir"])]);
        assert_eq!(created.paths, vec![PathBuf::from("dir")]);
        let removed = run(&rules, &[ev(ChangeKind::Removed, &["/proj/dir"])]);
        assert_eq!(removed.paths, vec![PathBuf::from("dir")]);
    }

    #[test]
    fn rename_reports_both_names() {
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[ev(ChangeKind::Renamed, &["/proj/old.txt", "/proj/new.txt"])],
        );
        assert_eq!(batch.total, 2);
    }

    #[test]
    fn unknown_location_is_reported_unless_it_is_inside_dot_git() {
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[ev(ChangeKind::Modified, &["/elsewhere/a.txt"])],
        );
        assert_eq!(batch.paths, vec![PathBuf::from("/elsewhere/a.txt")]);
    }

    #[test]
    fn overflow_requires_a_rescan_even_without_paths() {
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[RawEvent {
                kind: ChangeKind::Other,
                paths: Vec::new(),
                rescan: true,
            }],
        );
        assert!(batch.rescan);
        assert!(!batch.is_empty());
        assert!(batch.touches_gitignore());
    }

    #[test]
    fn caps_the_reported_paths_but_keeps_the_total() {
        let paths: Vec<String> = (0..MAX_REPORTED_PATHS + 25)
            .map(|i| format!("/proj/f{i:04}.txt"))
            .collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[ev(ChangeKind::Created, &refs)],
        );
        assert_eq!(batch.paths.len(), MAX_REPORTED_PATHS);
        assert_eq!(batch.total, MAX_REPORTED_PATHS + 25);
    }

    #[test]
    fn gitignore_change_is_detected() {
        let batch = run(
            &IgnoreRules::builtin_only(),
            &[ev(ChangeKind::Modified, &["/proj/.gitignore"])],
        );
        assert!(batch.touches_gitignore());
        let other = run(
            &IgnoreRules::builtin_only(),
            &[ev(ChangeKind::Modified, &["/proj/sub/.gitignore"])],
        );
        assert!(!other.touches_gitignore());
    }
}
