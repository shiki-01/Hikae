// プロジェクトのフォルダのファイル監視（notify + notify-debouncer-full。設計書 8.2、9.2）。
//
// OS からの変更通知を `DEBOUNCE` の間まとめ（静止判定）、`changes::summarize` で保存の対象に
// なりうる変更だけに絞って、コールバックへ渡す。判断は純粋なロジック（`changes`、`ignore_rules`）で、
// ここは OS の通知を `RawEvent` に変換して橋渡しするだけの薄い層。
// 監視自体が失敗しても（フォルダの消失など）アプリは落とさず、`WatchEvent::Failed` で知らせる。

use crate::changes::{summarize, ChangeBatch, ChangeKind, RawEvent};
use crate::ignore_rules::IgnoreRules;
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

/// 変更通知をまとめる時間（設計書 9.2「300ms で間引き」）
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// 監視を始められなかった
#[derive(Error, Debug)]
pub enum WatchError {
    #[error("watch error: {0}")]
    Notify(#[from] notify::Error),
    #[error("watch target is not a directory: {0}")]
    NotADirectory(PathBuf),
}

/// 監視からの通知
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// 保存の対象になりうる変更があった（空のまとめは通知しない）
    Changed(ChangeBatch),
    /// 監視でエラーが起きた（以降の通知は信頼できない。呼び出し側が監視をやり直す）
    Failed(String),
}

/// notify の種類を判断に必要な分類にする
fn change_kind(kind: &EventKind) -> ChangeKind {
    use notify::event::ModifyKind;
    match kind {
        EventKind::Access(_) => ChangeKind::Access,
        EventKind::Create(_) => ChangeKind::Created,
        EventKind::Remove(_) => ChangeKind::Removed,
        EventKind::Modify(ModifyKind::Name(_)) => ChangeKind::Renamed,
        EventKind::Modify(_) | EventKind::Any => ChangeKind::Modified,
        EventKind::Other => ChangeKind::Other,
    }
}

/// 1 つのプロジェクトのフォルダを監視する。値を破棄すると監視を止める。
pub struct ProjectWatcher {
    _debouncer: Debouncer<RecommendedWatcher, NoCache>,
}

impl ProjectWatcher {
    /// `root` の監視を始める。`on_event` は監視のスレッドから呼ばれる（重い処理をしないこと）。
    pub fn start(
        root: &Path,
        on_event: impl Fn(WatchEvent) + Send + 'static,
    ) -> Result<Self, WatchError> {
        if !root.is_dir() {
            return Err(WatchError::NotADirectory(root.to_path_buf()));
        }
        // OS が返すパスはシンボリックリンクなどを解決した形のことがある（macOS の /var など）。
        // 相対パスへの変換は、与えられた形と正規化した形の両方で試す
        let mut roots = vec![root.to_path_buf()];
        if let Ok(canonical) = root.canonicalize() {
            if canonical != root {
                roots.push(canonical);
            }
        }
        let project_root = root.to_path_buf();
        let mut rules = IgnoreRules::load(root);

        // ファイル ID のキャッシュ（`RecommendedCache`）は、監視の開始時にフォルダ全体を走査するため使わない。
        // 名前変更の From / To を 1 件にまとめられなくなるが、必要なのは変更されたパスだけで支障はない
        let handler = move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    let raw: Vec<RawEvent> = events
                        .iter()
                        .map(|e| RawEvent {
                            kind: change_kind(&e.event.kind),
                            paths: e.event.paths.clone(),
                            rescan: e.event.need_rescan(),
                        })
                        .collect();
                    let batch = summarize(&rules, &roots, &raw, |p| p.is_dir());
                    if batch.is_empty() {
                        return;
                    }
                    // .gitignore が変わったら、次の判定から新しい規則を使う
                    if batch.touches_gitignore() {
                        rules = IgnoreRules::load(&project_root);
                    }
                    on_event(WatchEvent::Changed(batch));
                }
                Err(errors) => {
                    let message = errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ");
                    on_event(WatchEvent::Failed(message));
                }
            }
        };
        let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            DEBOUNCE,
            None,
            handler,
            NoCache,
            notify::Config::default(),
        )?;
        debouncer.watch(root, RecursiveMode::Recursive)?;
        Ok(ProjectWatcher {
            _debouncer: debouncer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};

    /// 通知を待つ上限。経過時間のしきい値の判定ではなく、通知が来ないときにテストを止めるためだけに使う
    const WAIT_LIMIT: Duration = Duration::from_secs(30);

    /// 条件を満たす通知が来るまで受け取る。来なければ（上限まで待って）None
    fn wait_for(
        rx: &Receiver<WatchEvent>,
        mut done: impl FnMut(&WatchEvent) -> bool,
    ) -> Option<Vec<WatchEvent>> {
        let mut seen = Vec::new();
        while let Ok(event) = rx.recv_timeout(WAIT_LIMIT) {
            let finished = done(&event);
            seen.push(event);
            if finished {
                return Some(seen);
            }
        }
        None
    }

    fn mentions(event: &WatchEvent, name: &str) -> bool {
        match event {
            WatchEvent::Changed(batch) => batch
                .paths
                .iter()
                .any(|p| p.to_string_lossy().contains(name)),
            WatchEvent::Failed(_) => false,
        }
    }

    #[test]
    fn creating_a_file_is_reported_with_a_relative_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (tx, rx) = channel();
        let _watcher = ProjectWatcher::start(tmp.path(), move |e| {
            let _ = tx.send(e);
        })
        .expect("start");

        std::fs::write(tmp.path().join("報告書.txt"), "hello").expect("write");

        let seen = wait_for(&rx, |e| mentions(e, "報告書.txt")).expect("通知が届く");
        let WatchEvent::Changed(batch) = seen.last().expect("last") else {
            panic!("変更の通知のはず");
        };
        // プロジェクトからの相対パスで届く
        assert!(
            batch.paths.contains(&PathBuf::from("報告書.txt")),
            "{batch:?}"
        );
    }

    #[test]
    fn ignored_files_are_not_reported_but_later_changes_still_are() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join(".git")).expect("mkdir");
        std::fs::write(tmp.path().join(".gitignore"), "*.bak\n").expect("write");
        let (tx, rx) = channel();
        let _watcher = ProjectWatcher::start(tmp.path(), move |e| {
            let _ = tx.send(e);
        })
        .expect("start");

        // 無視されるものを先に作り、最後に対象のファイルを作る。対象の通知が届くまでに
        // 無視されるべきパスが 1 つも現れないことを確かめる
        std::fs::write(tmp.path().join(".git").join("index.lock"), "x").expect("write");
        std::fs::write(tmp.path().join("~$a.docx"), "x").expect("write");
        std::fs::write(tmp.path().join("old.bak"), "x").expect("write");
        std::fs::write(tmp.path().join("real.txt"), "x").expect("write");

        let seen = wait_for(&rx, |e| mentions(e, "real.txt")).expect("通知が届く");
        for event in &seen {
            for ignored in [".git", "~$a.docx", "old.bak"] {
                assert!(
                    !mentions(event, ignored),
                    "{ignored} が通知された: {event:?}"
                );
            }
        }
    }

    #[test]
    fn starting_on_a_missing_folder_fails_without_panicking() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("nothing-here");
        let result = ProjectWatcher::start(&missing, |_| {});
        assert!(matches!(result, Err(WatchError::NotADirectory(_))));
    }
}
