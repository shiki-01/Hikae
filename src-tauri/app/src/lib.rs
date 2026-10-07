// Hikae の Tauri 層。コマンド・イベント、async操作キュー、状態管理を行う。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder};

use core_git::GitRunner;
use core_ops::{Ops, OpsError};
use core_store::{Project, ProjectLocks, Store};
use core_watch::SyncTask;

mod events;
mod ops_runner;
mod scheduler;

use events::OpTrigger;
use ops_runner::{
    memo_labels, pull_outcome_name, push_outcome_name, run_op, summarize_pull, summarize_push,
    OpContext, OpSpec, OpSummary,
};
use scheduler::{pull_task_result, push_task_result, SchedulerHandle};

// ========== エラー型（specta::Type 実装、設計書5章） ==========

/// 何が起きたか、データは無事か、次の行動を含むエラー型
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AppError {
    /// 何が起きたか（ユーザー向けの平易な説明）
    pub what_happened: String,
    /// データは無事か（1文で）
    pub data_is_safe: String,
    /// 次の行動（ボタンラベルと説明）
    pub next_action: String,
    /// 技術情報（Git エラー原文など。秘密情報は除外）
    pub technical_info: Option<String>,
}

impl AppError {
    pub(crate) fn from_ops_error(e: OpsError) -> Self {
        let (what_happened, data_is_safe, next_action, technical_info) = match e {
            OpsError::Conflict(files) => (
                format!(
                    "変更のぶつかり: {} 件のファイルが2台で別々に変更されました",
                    files.len()
                ),
                "ファイルは安全に保存されています。".to_string(),
                "競合を解消してください".to_string(),
                Some(format!(
                    "{} files: {:?}",
                    files.len(),
                    files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )),
            ),
            OpsError::Git(e) => (
                "Git 操作に失敗しました".to_string(),
                "このパソコンのファイルは安全に残っています。".to_string(),
                "もう一度試すか、詳細を確認してください".to_string(),
                Some(format!("{:?}", e)),
            ),
            OpsError::Safety(e) => (
                "安全性チェックに失敗しました".to_string(),
                "ファイルは変更されていません。".to_string(),
                "詳細を確認してください".to_string(),
                Some(format!("{:?}", e)),
            ),
            OpsError::Io(e) => (
                "ファイルアクセスエラーが発生しました".to_string(),
                "ディスク容量や権限を確認してください".to_string(),
                "詳細を確認してください".to_string(),
                Some(e.to_string()),
            ),
            OpsError::Unexpected(msg) => (
                "予期しないエラーが発生しました".to_string(),
                "データは安全に保存されています".to_string(),
                "サポートに連絡してください".to_string(),
                Some(msg),
            ),
        };

        AppError {
            what_happened,
            data_is_safe,
            next_action,
            technical_info,
        }
    }
}

// ========== AppState ==========

/// アプリケーション状態
pub struct AppState {
    /// SQLite データベース（Store）
    pub store: Arc<Mutex<Store>>,
    /// プロジェクト ID ごとの操作キュー（状態変更の直列実行）
    pub locks: Arc<ProjectLocks>,
    /// 起動時・定期の取り込みとアップロードのスケジューラ
    pub scheduler: SchedulerHandle,
}

impl AppState {
    /// 新規作成
    pub fn new(store: Store) -> Self {
        AppState {
            store: Arc::new(Mutex::new(store)),
            locks: Arc::new(ProjectLocks::new()),
            scheduler: SchedulerHandle::new(),
        }
    }

    /// ロックを複製（Clone用）
    pub fn locks_clone(&self) -> Arc<ProjectLocks> {
        self.locks.clone()
    }

    /// ストアを複製（Clone用）
    pub fn store_clone(&self) -> Arc<Mutex<Store>> {
        self.store.clone()
    }
}

/// 同一プロジェクトへの状態変更を直列実行する（安全上の不変条件 7）。
///
/// ロックは spawn_blocking の内側で取る。外側で `locks.run` に JoinHandle を
/// 作らせると、クロージャが即座に戻ってロックが本体の実行前に解放されてしまう。
pub(crate) async fn run_exclusive<T, F>(
    locks: Arc<ProjectLocks>,
    id: String,
    data_is_safe: &'static str,
    f: F,
) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    let joined = tauri::async_runtime::spawn_blocking(move || locks.run(&id, f))
        .await
        .map_err(|e| AppError {
            what_happened: "タスク実行に失敗しました".to_string(),
            data_is_safe: data_is_safe.to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;
    joined.map_err(|e| AppError {
        what_happened: "ロック取得に失敗しました".to_string(),
        data_is_safe: data_is_safe.to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e),
    })?
}

/// 読み取り専用の処理を blocking スレッドで実行する（直列キューは通さない）。
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError {
            what_happened: "タスク実行に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?
}

/// ID でプロジェクトを取得する（ストアのロックは取得後すぐ解放する）。
fn load_project(store: &Arc<Mutex<Store>>, id: &str) -> Result<Project, AppError> {
    let guard = store.lock().map_err(|e| AppError {
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?;
    guard.get_project(id).map_err(|e| AppError {
        what_happened: "プロジェクトが見つかりません".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "プロジェクト一覧から確認してください".to_string(),
        technical_info: Some(format!("{:?}", e)),
    })
}

// ========== Tauri コマンド（全て async/spawn_blocking） ==========

/// プロジェクト一覧を取得。
#[tauri::command]
#[specta::specta]
async fn list_projects(state: tauri::State<'_, AppState>) -> Result<Vec<ProjectInfo>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "プロジェクト一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let projects = store_guard.list_projects().map_err(|e| AppError {
            what_happened: "プロジェクト一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        Ok(projects
            .into_iter()
            .map(|p| ProjectInfo {
                id: p.id,
                display_name: p.display_name,
                path: p.path,
                remote_url: p.remote_url,
                owner: p.owner,
                last_viewed_at: p.last_viewed_at,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// プロジェクトを追加（既存フォルダ登録・新規作成・GitHub から clone）。
#[tauri::command]
#[specta::specta]
async fn add_project(
    state: tauri::State<'_, AppState>,
    id: String,
    display_name: String,
    path: PathBuf,
    owner: String,
    remote_url: Option<String>,
) -> Result<(), AppError> {
    let store = state.store.clone();
    let locks = state.locks.clone();
    let id_clone = id.clone();
    run_exclusive(
        locks,
        id,
        "ファイルは変更されていません",
        move || {
            let runner = GitRunner::from_path_env();
            let ops = Ops::new(runner);
            let identity = core_ops::Identity {
                name: owner.clone(),
                email: format!("{}+{}@users.noreply.github.com", id_clone, owner),
            };

            ops.init_project(&path, remote_url.as_deref(), &identity)
                .map_err(AppError::from_ops_error)?;

            let store_guard = store.lock().map_err(|e| AppError {
                what_happened: "データベースアクセスに失敗しました".to_string(),
                data_is_safe: "ファイルは安全です".to_string(),
                next_action: "もう一度試してください".to_string(),
                technical_info: Some(e.to_string()),
            })?;

            store_guard
                .add_project(
                    &id_clone,
                    &display_name,
                    &path,
                    remote_url.as_deref(),
                    &owner,
                    "main",
                )
                .map_err(|e| AppError {
                    what_happened: "プロジェクト登録に失敗しました".to_string(),
                    data_is_safe: "ファイルは変更されていません".to_string(),
                    next_action: "もう一度試してください".to_string(),
                    technical_info: Some(format!("{:?}", e)),
                })?;

            Ok(())
        },
    )
    .await
}

/// プロジェクトを削除（登録のみ。フォルダは消さない）。
#[tauri::command]
#[specta::specta]
async fn remove_project(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "データベースアクセスに失敗しました".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        store_guard.remove_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクト削除に失敗しました".to_string(),
            data_is_safe: "フォルダ内のファイルは残っています".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        Ok(())
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// プロジェクトの状態を取得（未保存の変更、アップロード待ち、競合など）。
#[tauri::command]
#[specta::specta]
async fn project_status(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<SyncStatus, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "状態取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);

        let sync = ops
            .sync_state(&project.path)
            .map_err(AppError::from_ops_error)?;
        let conflicts = ops
            .conflicts(&project.path)
            .map_err(AppError::from_ops_error)?;

        Ok(SyncStatus {
            unsaved_changes: 0,
            upload_pending: sync.ahead,
            pull_pending: sync.behind,
            has_conflicts: !conflicts.is_empty(),
            is_syncing: false,
        })
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 変更ファイル一覧を取得。
#[tauri::command]
#[specta::specta]
async fn list_changes(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<ChangeFile>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "変更一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();

        let out = runner
            .run_ok(
                &project.path,
                &["status", "--porcelain=v2", "-z", "--branch"],
            )
            .map_err(|e| AppError {
                what_happened: "変更一覧の取得に失敗しました".to_string(),
                data_is_safe: "ファイルは安全です".to_string(),
                next_action: "もう一度試してください".to_string(),
                technical_info: Some(format!("{:?}", e)),
            })?;

        let output = String::from_utf8_lossy(&out.stdout);
        let mut changes = Vec::new();

        for line in output.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let status = parts[1];
            let kind = match status {
                "M" => ChangeKind::Modified,
                "A" => ChangeKind::Added,
                "D" => ChangeKind::Deleted,
                "R" => ChangeKind::Renamed,
                _ => continue,
            };

            if let Some(last_part) = parts.last() {
                changes.push(ChangeFile {
                    path: last_part.to_string(),
                    kind,
                });
            }
        }

        Ok(changes)
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 保存（commit）を実行。
#[tauri::command]
#[specta::specta]
async fn save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    message: String,
) -> Result<SaveResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let store = state.store_clone();
    let touch_id = id.clone();
    let memo = message.clone();

    let outcome = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "save",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "変更は保存されていません",
        },
        move |ops, path| {
            let outcome = ops.save(path, &memo)?;
            if let Ok(guard) = store.lock() {
                let _ = guard.touch_project(&touch_id);
            }
            Ok(outcome)
        },
        |o| OpSummary {
            detail: match o {
                core_ops::SaveOutcome::Saved { .. } => "saved",
                core_ops::SaveOutcome::NothingToSave => "nothing-to-save",
            }
            .to_string(),
            quiet: false,
        },
    )
    .await?;

    let commit = match outcome {
        core_ops::SaveOutcome::Saved { commit, .. } => Some(commit),
        core_ops::SaveOutcome::NothingToSave => None,
    };

    // 「保存時に自動アップロード」がオンなら、スケジューラがキュー経由でアップロードする
    if commit.is_some() {
        state.scheduler.request_push(&id);
    }

    Ok(SaveResult {
        commit,
        message: Some(message),
    })
}

/// 未保存の変更から保存メモの案を作る（ルールベース、設計書 10.3）。
/// 読み取りのみのため直列キューは通さない。
#[tauri::command]
#[specta::specta]
async fn suggest_memo(state: tauri::State<'_, AppState>, id: String) -> Result<String, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(GitRunner::from_path_env());
        ops.suggest_memo(&project.path, &memo_labels())
            .map_err(AppError::from_ops_error)
    })
    .await
}

/// 指定した保存時点の全ファイル一覧（読み取りのみ）。
#[tauri::command]
#[specta::specta]
async fn list_files_at(
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
) -> Result<Vec<FileEntry>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(GitRunner::from_path_env());
        let files = ops
            .list_files_at(&project.path, &commit)
            .map_err(AppError::from_ops_error)?;
        Ok(files
            .into_iter()
            .map(|f| FileEntry {
                path: f.path,
                size: f.size as f64,
            })
            .collect())
    })
    .await
}

/// 競合ファイルを UI 向けの型へ変換する
fn conflict_item(f: core_ops::ConflictFile) -> ConflictItem {
    ConflictItem {
        path: f.path,
        this_saved_at: f.this_saved_at.map(|t| t as f64),
        cloud_saved_at: f.cloud_saved_at.map(|t| t as f64),
        kind: match f.kind {
            core_ops::ConflictKind::BothModified => ConflictKind::BothModified,
            core_ops::ConflictKind::BothAdded => ConflictKind::BothAdded,
            core_ops::ConflictKind::DeletedByUs => ConflictKind::DeletedByUs,
            core_ops::ConflictKind::DeletedByThem => ConflictKind::DeletedByThem,
            core_ops::ConflictKind::BothDeleted => ConflictKind::BothDeleted,
        },
    }
}

/// 取り込む（fetch + merge）を実行。
#[tauri::command]
#[specta::specta]
async fn pull(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<PullResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let result = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "pull",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        |ops, path| ops.pull(path),
        summarize_pull,
    )
    .await;
    // 手動の結果もスケジューラの計画に反映する（成功すれば失敗状態が解除される）
    state
        .scheduler
        .note_result(&id, SyncTask::Pull, pull_task_result(&result));
    let outcome = result?;

    let name = pull_outcome_name(&outcome).to_string();
    let conflicts = match outcome {
        core_ops::PullOutcome::Conflicted { files } => {
            files.into_iter().map(conflict_item).collect()
        }
        _ => vec![],
    };
    Ok(PullResult {
        outcome: name,
        conflicts,
    })
}

/// アップロード（push）を実行。
#[tauri::command]
#[specta::specta]
async fn push(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<PushResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let result = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "push",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        |ops, path| ops.upload(path),
        summarize_push,
    )
    .await;
    state
        .scheduler
        .note_result(&id, SyncTask::Push, push_task_result(&result));
    let outcome = result?;

    Ok(PushResult {
        outcome: push_outcome_name(&outcome).to_string(),
    })
}

/// 履歴を取得。
#[tauri::command]
#[specta::specta]
async fn list_history(
    state: tauri::State<'_, AppState>,
    id: String,
    max_count: u32,
) -> Result<Vec<HistoryItem>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "履歴取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);

        let entries = ops
            .history(&project.path, max_count as usize)
            .map_err(AppError::from_ops_error)?;

        Ok(entries
            .into_iter()
            .map(|e| HistoryItem {
                commit: e.commit,
                timestamp: e.timestamp,
                message: e.message,
                changed_files_count: e.changed_files_count,
                is_snapshot: e.snapshot_ref.is_some(),
            })
            .collect())
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 差分を取得（2つの時点の比較）。
#[tauri::command]
#[specta::specta]
async fn diff(
    state: tauri::State<'_, AppState>,
    id: String,
    from: String,
    to: String,
    path: Option<String>,
) -> Result<Vec<DiffLine>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "差分取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);

        let lines = ops
            .diff_with(&project.path, &from, &to, path.as_deref())
            .map_err(AppError::from_ops_error)?;

        Ok(lines
            .into_iter()
            .map(|l| DiffLine {
                kind: match l.kind {
                    core_ops::DiffLineKind::Added => DiffLineKind::Added,
                    core_ops::DiffLineKind::Removed => DiffLineKind::Removed,
                    core_ops::DiffLineKind::Context => DiffLineKind::Context,
                },
                content: l.content,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 元に戻す操作のプレビュー（影響ファイル一覧）。
#[tauri::command]
#[specta::specta]
async fn restore_preview(
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
) -> Result<RestorePreviewData, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "プレビュー生成に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);

        let preview = ops
            .restore_preview(&project.path, &commit)
            .map_err(AppError::from_ops_error)?;

        Ok(RestorePreviewData {
            modified: preview.modified.into_iter().map(|c| c.path).collect(),
            deleted: preview.deleted,
            created: preview.created,
        })
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 元に戻す実行。
#[tauri::command]
#[specta::specta]
async fn restore(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    let target = commit.clone();
    run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "restore",
            trigger: OpTrigger::Manual,
            target: Some(target),
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, path| ops.restore(path, &commit),
        |_| OpSummary {
            detail: "restored".to_string(),
            quiet: false,
        },
    )
    .await?;
    Ok(())
}

/// 現在の競合ファイルを取得。
#[tauri::command]
#[specta::specta]
async fn list_conflicts(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<ConflictItem>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            what_happened: "競合一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);

        let conflicts = ops
            .conflicts(&project.path)
            .map_err(AppError::from_ops_error)?;

        Ok(conflicts
            .into_iter()
            .map(|f| ConflictItem {
                path: f.path,
                this_saved_at: f.this_saved_at.map(|t| t as f64),
                cloud_saved_at: f.cloud_saved_at.map(|t| t as f64),
                kind: match f.kind {
                    core_ops::ConflictKind::BothModified => ConflictKind::BothModified,
                    core_ops::ConflictKind::BothAdded => ConflictKind::BothAdded,
                    core_ops::ConflictKind::DeletedByUs => ConflictKind::DeletedByUs,
                    core_ops::ConflictKind::DeletedByThem => ConflictKind::DeletedByThem,
                    core_ops::ConflictKind::BothDeleted => ConflictKind::BothDeleted,
                },
            })
            .collect())
    })
    .await
    .map_err(|e| AppError {
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 競合を解消。
#[tauri::command]
#[specta::specta]
async fn resolve_conflicts(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    choices: Vec<(String, ConflictChoice)>,
    keep_other_copy: bool,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    let converted_choices: Vec<(String, core_ops::Choice)> = choices
        .into_iter()
        .map(|(path, choice)| {
            let c = match choice {
                ConflictChoice::Mine => core_ops::Choice::Mine,
                ConflictChoice::Theirs => core_ops::Choice::Theirs,
            };
            (path, c)
        })
        .collect();
    let message = "2台の変更をまとめました".to_string();

    run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "resolve",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, path| ops.resolve(path, &converted_choices, keep_other_copy, &message),
        |_| OpSummary {
            detail: "resolved".to_string(),
            quiet: false,
        },
    )
    .await?;

    // 解消後は状態を確かめる取り込みと、まとめた保存のアップロードを予定する
    state.scheduler.request_pull(&id);
    state.scheduler.request_push(&id);
    Ok(())
}

// ========== Data Types (Tauri-Specta 用) ==========

/// プロジェクト情報
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ProjectInfo {
    pub id: String,
    pub display_name: String,
    pub path: PathBuf,
    pub remote_url: Option<String>,
    pub owner: String,
    pub last_viewed_at: String,
}

/// 変更ファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChangeFile {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub enum ChangeKind {
    #[serde(rename = "modified")]
    Modified,
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "renamed")]
    Renamed,
}

/// 保存結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SaveResult {
    pub commit: Option<String>,
    pub message: Option<String>,
}

/// 取り込み結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct PullResult {
    pub outcome: String,
    pub conflicts: Vec<ConflictItem>,
}

/// アップロード結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct PushResult {
    pub outcome: String,
}

/// 履歴アイテム
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct HistoryItem {
    pub commit: String,
    pub timestamp: String,
    pub message: String,
    pub changed_files_count: u32,
    pub is_snapshot: bool,
}

/// 差分行
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub enum DiffLineKind {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "removed")]
    Removed,
    #[serde(rename = "context")]
    Context,
}

/// 元に戻すプレビュー
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct RestorePreviewData {
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub created: Vec<String>,
}

/// 競合ファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ConflictItem {
    pub path: String,
    pub kind: ConflictKind,
    /// この PC 側の最終保存日時（Unix 秒）。不明なら null
    pub this_saved_at: Option<f64>,
    /// クラウド側の最終保存日時（Unix 秒）。不明なら null
    pub cloud_saved_at: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub enum ConflictKind {
    #[serde(rename = "both-modified")]
    BothModified,
    #[serde(rename = "both-added")]
    BothAdded,
    #[serde(rename = "deleted-by-us")]
    DeletedByUs,
    #[serde(rename = "deleted-by-them")]
    DeletedByThem,
    #[serde(rename = "both-deleted")]
    BothDeleted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub enum ConflictChoice {
    #[serde(rename = "mine")]
    Mine,
    #[serde(rename = "theirs")]
    Theirs,
}

/// 保存時点のファイル（S3「この時点の全ファイルを見る」用）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileEntry {
    /// リポジトリ相対パス
    pub path: String,
    /// ファイルサイズ（バイト）
    pub size: f64,
}

/// 同期状態
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SyncStatus {
    pub unsaved_changes: u32,
    pub upload_pending: u32,
    pub pull_pending: u32,
    pub has_conflicts: bool,
    pub is_syncing: bool,
}

// ========== Tauri Setup ==========

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            list_projects,
            add_project,
            remove_project,
            project_status,
            list_changes,
            save,
            pull,
            push,
            list_history,
            diff,
            restore_preview,
            restore,
            list_conflicts,
            resolve_conflicts,
            suggest_memo,
            list_files_at,
        ])
        .events(collect_events![
            events::StatusChanged,
            events::OpProgress,
            events::OpFinished,
            events::SyncStateChanged,
            events::NeedsAttention,
        ])
}

/// src/lib/bindings.ts を書き出す（debug 起動時と export-bindings bin から呼ぶ）
pub fn export_bindings() {
    specta_builder()
        .export(
            specta_typescript::Typescript::default(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/bindings.ts"),
        )
        .expect("TS 型の書き出しに失敗しました");
}

pub fn run() {
    let builder = specta_builder();
    let invoke_handler = builder.invoke_handler();

    #[cfg(debug_assertions)]
    export_bindings();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // イベントの登録（UI への通知に必要）
            builder.mount_events(app);

            // app_data_dir 内に DB を配置
            let app_data_dir = app.path().app_data_dir().map_err(|e| {
                tauri::Error::Io(std::io::Error::other(format!(
                    "Failed to get app data dir: {}",
                    e
                )))
            })?;

            std::fs::create_dir_all(&app_data_dir).map_err(tauri::Error::Io)?;

            let db_path = app_data_dir.join("hikae.db");

            // Store を初期化
            let store = core_store::Store::open(&db_path).map_err(|e| {
                tauri::Error::Io(std::io::Error::other(format!(
                    "Failed to open database: {:?}",
                    e
                )))
            })?;

            let state = AppState::new(store);
            app.manage(state);

            // 起動時・定期の取り込みとアップロードを開始（実行は操作キュー経由）
            scheduler::spawn(app.handle().clone());

            Ok(())
        })
        .invoke_handler(invoke_handler)
        .run(tauri::generate_context!())
        .expect("アプリの起動に失敗しました");
}
