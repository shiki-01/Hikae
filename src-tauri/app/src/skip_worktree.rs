// 「今後の変更だけ無視する」（skip-worktree、設計書 4.8、S6）の画面向けコマンドと、起動時の再適用。
//
// - 印の付け外しは `run_op`（直列キュー・ジャーナル）を通す。記録（この PC の SQLite）は git 側の操作が
//   成功したあとに更新する（git 側が正。記録の更新に失敗しても、git の状態は戻さない）
// - 記録と現在の状態、再適用の提案の判定は読み取りのため `run_blocking`（キューは通さない）
// - 取り込み・解消・中止・復旧の結果の通知は `run_op` が出す（`SkipWorktreeNotice`）

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use core_ops::Ops;
use core_store::{now_rfc3339, SkipWorktreeRecord, Store};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::events::OpTrigger;
use crate::ops_runner::{run_op, OpContext, OpSpec, OpSummary};
use crate::{load_project, run_blocking, AppError, AppState};

/// 「今後の変更だけ無視する」の記録 1 件（S6 の「この PC だけ無視中のファイル」）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SkipWorktreeItem {
    /// プロジェクトからの相対パス（`/` 区切り）
    pub path: String,
    /// 無視を指定した時刻（RFC 3339）
    pub set_at: String,
    /// いまこの PC で無視されているか。false なら無視が外れている（再適用の提案の対象）
    pub ignored: bool,
    /// 再適用の提案で「今はしない」を選んだ（再度は提案しない）
    pub dismissed: bool,
}

/// 印の付け外しの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipChange {
    Set,
    Clear,
}

fn lock_error(e: impl std::fmt::Display) -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: "ファイルは変更されていません".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    }
}

fn store_failure(e: core_store::StoreError) -> AppError {
    lock_error(e)
}

/// プロジェクトの記録（パスの順）。ストアのロックは読み終えたらすぐ解放する
fn records(store: &Arc<Mutex<Store>>, id: &str) -> Result<Vec<SkipWorktreeRecord>, AppError> {
    let guard = store.lock().map_err(lock_error)?;
    guard.list_skip_worktree(id).map_err(store_failure)
}

/// 「今後の変更だけ無視する」の記録と、この PC での現在の状態（読み取りのみ）
#[tauri::command]
#[specta::specta]
pub async fn list_skip_worktree(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<SkipWorktreeItem>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let recorded = records(&store, &id)?;
        let ignored: BTreeSet<String> = Ops::new(crate::git_runner())
            .list_skip_worktree(&project.path)
            .map_err(AppError::from_ops_error)?
            .into_iter()
            .collect();
        Ok(recorded
            .into_iter()
            .map(|r| SkipWorktreeItem {
                ignored: ignored.contains(&r.path),
                path: r.path,
                set_at: r.set_at,
                dismissed: r.dismissed,
            })
            .collect())
    })
    .await
}

/// 無視が外れている記録のうち、まだ提案していないもの（読み取りのみ。設計書 4.8 の 3）。
/// プロジェクトを開いたときに、画面が一度だけ提案するために使う。
#[tauri::command]
#[specta::specta]
pub async fn list_skip_worktree_drift(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let recorded = records(&store, &id)?;
        let all: Vec<String> = recorded.iter().map(|r| r.path.clone()).collect();
        if all.is_empty() {
            return Ok(Vec::new());
        }
        let undismissed: BTreeSet<String> = recorded
            .iter()
            .filter(|r| !r.dismissed)
            .map(|r| r.path.clone())
            .collect();
        let drift = Ops::new(crate::git_runner())
            .list_skip_worktree_drift(&project.path, &all)
            .map_err(AppError::from_ops_error)?;
        Ok(drift
            .into_iter()
            .filter(|p| undismissed.contains(p))
            .collect())
    })
    .await
}

/// 再適用の提案で「今はしない」を選んだ。記録の印を立て、同じ提案を繰り返さない（作業フォルダは変えない）。
#[tauri::command]
#[specta::specta]
pub async fn dismiss_skip_worktree_drift(
    state: tauri::State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> Result<(), AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let guard = store.lock().map_err(lock_error)?;
        guard
            .set_skip_worktree_dismissed(&id, &paths, true)
            .map_err(store_failure)
    })
    .await
}

/// 印の付け外しを、直列キューの中で行う。成功したあとに記録を更新する。
async fn change_skip_worktree(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    change: SkipChange,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    let store = ctx.store.clone();
    let (operation, data_is_safe) = match change {
        SkipChange::Set => (
            "skip-worktree-set",
            "ファイルは変わっていません（今後の変更を保存しないように設定するだけです）",
        ),
        SkipChange::Clear => (
            "skip-worktree-clear",
            "ファイルは変わっていません（無視をやめるだけです）",
        ),
    };
    let record_id = id.clone();
    let record_paths = paths.clone();
    run_op(
        &ctx,
        &id,
        OpSpec {
            operation,
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe,
        },
        move |ops, path| {
            match change {
                SkipChange::Set => ops.set_skip_worktree(path, &paths)?,
                SkipChange::Clear => ops.clear_skip_worktree(path, &paths)?,
            }
            // 記録は git の操作の後に更新する。記録の更新に失敗しても、git の状態は戻さない
            if let Ok(guard) = store.lock() {
                let _ = match change {
                    SkipChange::Set => {
                        guard.add_skip_worktree(&record_id, &record_paths, &now_rfc3339())
                    }
                    SkipChange::Clear => guard
                        .remove_skip_worktree(&record_id, &record_paths)
                        .map(|_| ()),
                };
            }
            Ok(())
        },
        |_: &()| OpSummary {
            detail: "changed".to_string(),
            quiet: false,
        },
    )
    .await?;
    Ok(())
}

/// 今後の変更を無視する（印を付ける）。追跡中のファイルだけ。復元点を作る。
#[tauri::command]
#[specta::specta]
pub async fn set_skip_worktree(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> Result<(), AppError> {
    change_skip_worktree(app, state, id, paths, SkipChange::Set).await
}

/// 無視をやめる（印を外す）。手元の変更は通常の未保存の変更として現れる。復元点を作る。
#[tauri::command]
#[specta::specta]
pub async fn clear_skip_worktree(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> Result<(), AppError> {
    change_skip_worktree(app, state, id, paths, SkipChange::Clear).await
}

/// 無視が外れている記録を、もう一度無視する（再適用の提案を承諾した）。`set_skip_worktree` と同じ処理。
#[tauri::command]
#[specta::specta]
pub async fn reapply_skip_worktree(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> Result<(), AppError> {
    change_skip_worktree(app, state, id, paths, SkipChange::Set).await
}

/// 起動時に、各プロジェクトの退避を書き戻して印を付け直す（設計書 4.8）。
/// 取り込みの途中で終わった場合や、書き戻せなかった退避を、次の起動で片付ける。退避が無ければ git は呼ばない。
/// 直列キューを通す。結果の通知は `run_op` が出す（ジャーナルには残さない）。
pub(crate) async fn reapply_on_startup(app: tauri::AppHandle) {
    let ids: Vec<String> = {
        let state = app.state::<AppState>();
        let store = state.store_clone();
        let Ok(guard) = store.lock() else {
            return;
        };
        match guard.list_projects() {
            Ok(list) => list.into_iter().map(|p| p.id).collect(),
            Err(_) => return,
        }
    };
    for id in ids {
        let ctx = {
            let state = app.state::<AppState>();
            OpContext::new(app.clone(), &state)
        };
        let _ = run_op(
            &ctx,
            &id,
            OpSpec {
                operation: "skip-worktree-reapply",
                trigger: OpTrigger::Auto,
                target: None,
                data_is_safe: "ファイルは変わっていません",
            },
            |ops, path| ops.reapply_skip_worktree(path).map(|_| ()),
            |_: &()| OpSummary {
                detail: "reapplied".to_string(),
                quiet: true,
            },
        )
        .await;
    }
}
