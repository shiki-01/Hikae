// プロジェクトのフォルダの監視の開始・停止（設計書 8.2、9.2）。
//
// 何を無視するか、変更をどうまとめるかは tauri 非依存の `core_watch`（`ProjectWatcher`）が決める。
// ここは、登録済みのプロジェクトの増減・フォルダ不明（E11）・監視の失敗に合わせて監視を開始・停止し、
// 変更の通知をスケジューラ（自動保存の静止判定）と UI（`FilesChanged`）へ橋渡しするだけの薄い層。
// 監視が始められない・途中で失敗したときも、アプリは落とさず、一定時間後に開始をやり直す。
// そのあいだ UI は、フォールバックとして一定間隔で変更一覧を取り直す（監視中でないと分かるため）。

use std::collections::HashMap;
use std::path::PathBuf;

use core_watch::{ProjectWatcher, WatchEvent};
use tauri_specta::Event;

use crate::events::FilesChanged;
use crate::scheduler::SchedulerHandle;

/// 監視の開始に失敗した、または途中で失敗したあと、開始をやり直すまでの待ち時間（秒）
const RETRY_SECS: u64 = 60;

struct Entry {
    path: PathBuf,
    watcher: Option<ProjectWatcher>,
    /// これ以前には開始をやり直さない時刻（スケジューラの時刻軸）
    retry_at: u64,
}

/// 監視中のプロジェクトの一覧
#[derive(Default)]
pub(crate) struct WatchManager {
    entries: HashMap<String, Entry>,
}

impl WatchManager {
    /// 監視の対象（フォルダが見つかるプロジェクト）に合わせて、監視を開始・停止する。
    /// 監視の状態が変わったプロジェクトの ID を返す（UI に状態を取り直させるため）。
    pub fn sync(
        &mut self,
        app: &tauri::AppHandle,
        handle: &SchedulerHandle,
        targets: &[(String, PathBuf)],
        now: u64,
    ) -> Vec<String> {
        let mut changed = Vec::new();

        // 登録が外された、フォルダが見つからなくなった、付け替えられたプロジェクトの監視を止める
        let stale: Vec<String> = self
            .entries
            .iter()
            .filter(|(id, entry)| {
                !targets
                    .iter()
                    .any(|(tid, path)| tid == *id && *path == entry.path)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(entry) = self.entries.remove(&id) {
                if entry.watcher.is_some() {
                    changed.push(id.clone());
                }
            }
            handle.set_watching(&id, false);
        }

        for (id, path) in targets {
            let entry = self.entries.entry(id.clone()).or_insert_with(|| Entry {
                path: path.clone(),
                watcher: None,
                retry_at: 0,
            });

            // 監視の途中で失敗した（フォルダの消失、OS の通知の異常など）。止めてやり直す
            if entry.watcher.is_some() && handle.take_watch_failed(id) {
                entry.watcher = None;
                entry.retry_at = now.saturating_add(RETRY_SECS);
                handle.set_watching(id, false);
                changed.push(id.clone());
                continue;
            }
            if entry.watcher.is_some() || now < entry.retry_at {
                continue;
            }

            // 古い失敗の印が新しい監視を止めないよう、開始の前に消す
            let _ = handle.take_watch_failed(id);
            match start(app, handle, id, path) {
                Some(watcher) => {
                    entry.watcher = Some(watcher);
                    handle.set_watching(id, true);
                    changed.push(id.clone());
                }
                None => {
                    entry.retry_at = now.saturating_add(RETRY_SECS);
                    handle.set_watching(id, false);
                }
            }
        }
        changed
    }
}

/// 1 つのプロジェクトの監視を開始する。始められなければ None。
fn start(
    app: &tauri::AppHandle,
    handle: &SchedulerHandle,
    id: &str,
    path: &std::path::Path,
) -> Option<ProjectWatcher> {
    let app = app.clone();
    let handle = handle.clone();
    let project_id = id.to_string();
    ProjectWatcher::start(path, move |event| match event {
        WatchEvent::Changed(_) => {
            // 自動保存の静止判定を延ばし、UI に変更一覧を取り直させる
            handle.note_files_changed(&project_id);
            let _ = FilesChanged {
                project_id: project_id.clone(),
            }
            .emit(&app);
        }
        WatchEvent::Failed(_) => handle.mark_watch_failed(&project_id),
    })
    .ok()
}
