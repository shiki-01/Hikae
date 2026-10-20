// 作業コピー（別の案を試す場所。設計書 4.9）の Tauri コマンド。
//
// - 作る・切り替える・採用するは状態を変えるため、`run_op` の直列キュー（不変条件 7）と操作ジャーナルを通す。
//   操作名は `work_copy_create` / `work_copy_switch` / `work_copy_adopt`（6.3）。復元点は core-ops が作る（不変条件 3）
// - 表示名は SQLite の `work_copies` に持つ。記録に失敗しても操作の成否は変えない（表示名の無いものは「無題の案」）
// - 「しまう」は表示名の記録の印だけを変える。git には触れず、ブランチは削除しない（不変条件 1）
// - 設定 `work_copy_enabled` がオフのとき、作る・切り替える・採用するは拒否する（一覧と「しまう」は読み取り・記録だけなので拒否しない）

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use core_ops::{created_at_of_work_copy, Ops, OpsError, PullOutcome, WorkCopyInfo};
use core_store::work_copies::{normalize_display_name, DISPLAY_NAME_MAX_CHARS};
use core_store::{now_rfc3339, Store, WorkCopyRecord};
use serde::{Deserialize, Serialize};

use crate::events::OpTrigger;
use crate::ops_runner::{pull_outcome_name, run_op, summarize_pull, OpContext, OpSpec, OpSummary};
use crate::{conflict_item, load_project, run_blocking, AppError, AppState, PullResult};

/// 一覧の 1 件。`branch` は内部名で、画面の操作の識別子としてだけ使い、表示しない（設計書 2.1）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct WorkCopyItem {
    /// 内部名（Git のブランチ名。`wc-YYYYMMDD-HHMMSS`）。画面には出さない
    pub branch: String,
    /// 利用者が付けた表示名。元のものと、表示名の記録が無いものは null（画面が文言を補う）
    pub display_name: Option<String>,
    /// いまの作業コピーか（作業フォルダがこのブランチを指している）
    pub is_current: bool,
    /// 元の作業コピーか（画面は「元のまま」と出す）
    pub is_original: bool,
    /// 「しまう」で一覧の既定から隠しているか
    pub archived: bool,
    /// 作成時刻（UTC の RFC 3339）。元のものは null。画面が地域の時刻に直して出す
    pub created_at: Option<String>,
}

/// 一覧を組み立てる（純粋な合成。git の読み取りと記録の読み取りは呼び出し側）。
/// 並びは、元のもの → 作業コピー（作成の古い順）。しまったものは既定では除くが、いまの作業コピーは残す。
fn merge_list(
    infos: Vec<WorkCopyInfo>,
    records: &[WorkCopyRecord],
    include_archived: bool,
) -> Vec<WorkCopyItem> {
    let by_branch: HashMap<&str, &WorkCopyRecord> =
        records.iter().map(|r| (r.branch.as_str(), r)).collect();
    let mut items: Vec<WorkCopyItem> = infos
        .into_iter()
        .map(|info| {
            let record = by_branch.get(info.branch.as_str()).copied();
            WorkCopyItem {
                display_name: record.map(|r| r.display_name.clone()),
                archived: record.is_some_and(|r| r.archived),
                created_at: if info.is_original {
                    None
                } else {
                    record
                        .map(|r| r.created_at.clone())
                        .or_else(|| created_at_of_work_copy(&info.branch))
                },
                branch: info.branch,
                is_current: info.is_current,
                is_original: info.is_original,
            }
        })
        .filter(|item| include_archived || item.is_current || !item.archived)
        .collect();
    items.sort_by(|a, b| {
        (!a.is_original, a.created_at.as_deref(), a.branch.as_str()).cmp(&(
            !b.is_original,
            b.created_at.as_deref(),
            b.branch.as_str(),
        ))
    });
    items
}

/// 元の作業コピーの内部名。一覧の `is_original` が 1 件だけのときに限る（それ以外は選べないため拒否）
fn original_of(infos: &[WorkCopyInfo]) -> Result<String, OpsError> {
    let mut originals = infos.iter().filter(|w| w.is_original);
    match (originals.next(), originals.next()) {
        (Some(one), None) => Ok(one.branch.clone()),
        _ => Err(OpsError::InvalidInput(
            "the original work copy could not be identified".to_string(),
        )),
    }
}

/// 作業コピーの機能がオフなら拒否する（設計書 7.1。何も変えていない）。
/// 設定が読めないときも、安全側に倒して拒否する。
fn ensure_enabled(store: &Arc<Mutex<Store>>, id: &str) -> Result<(), AppError> {
    let enabled = store
        .lock()
        .ok()
        .and_then(|g| g.effective_settings(id).ok())
        .map(|s| s.work_copy_enabled)
        .unwrap_or(false);
    if enabled {
        Ok(())
    } else {
        Err(work_copy_disabled())
    }
}

fn work_copy_disabled() -> AppError {
    AppError {
        code: "work_copy_disabled".to_string(),
        params: Vec::new(),
        what_happened: "作業コピーの機能がオフになっています".to_string(),
        data_is_safe: "ファイルは変更されていません。".to_string(),
        next_action: "設定の「詳細」で「作業コピーを使う」をオンにしてから、もう一度お試しください"
            .to_string(),
        technical_info: None,
    }
}

fn name_invalid() -> AppError {
    AppError {
        code: "work_copy_name_invalid".to_string(),
        params: Vec::new(),
        what_happened: format!(
            "案の名前が使えない形式です（1〜{DISPLAY_NAME_MAX_CHARS} 文字。改行などは使えません）"
        ),
        data_is_safe: "ファイルは変更されていません。".to_string(),
        next_action: "名前を入れ直してから、もう一度お試しください".to_string(),
        technical_info: None,
    }
}

fn not_found() -> AppError {
    AppError {
        code: "work_copy_not_found".to_string(),
        params: Vec::new(),
        what_happened: "指定された作業コピーが見つかりませんでした".to_string(),
        data_is_safe: "ファイルは変更されていません。".to_string(),
        next_action: "一覧を開き直してから、もう一度お試しください".to_string(),
        technical_info: None,
    }
}

/// 取り込みの結果を画面の型へ変換する（取り込みと同じ型。ぶつかりの解消へ同じ経路で進む）
fn pull_result(outcome: PullOutcome) -> PullResult {
    let name = pull_outcome_name(&outcome).to_string();
    let conflicts = match outcome {
        PullOutcome::Conflicted { files } => files.into_iter().map(conflict_item).collect(),
        _ => Vec::new(),
    };
    PullResult {
        outcome: name,
        conflicts,
        size_check: None,
        unsaved_count: None,
    }
}

/// 作業コピーの一覧（読み取りのみ。記録と git の内容を合成する）。
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_work_copies(
    state: tauri::State<'_, AppState>,
    id: String,
    include_archived: bool,
) -> Result<Vec<WorkCopyItem>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let infos = Ops::new(crate::git_runner())
            .list_work_copies(&project.path)
            .map_err(AppError::from_ops_error)?;
        let records = store
            .lock()
            .map_err(|e| db_error(&e))?
            .list_work_copies(&id)
            .map_err(|e| db_error(&e))?;
        Ok(merge_list(infos, &records, include_archived))
    })
    .await
}

/// 「別の案を試す」。表示名を検査し、いまの版から作業コピーを作って切り替える。
#[tauri::command]
#[specta::specta]
pub(crate) async fn create_work_copy(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    display_name: String,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    ensure_enabled(&ctx.store, &id)?;
    let name = normalize_display_name(&display_name).ok_or_else(name_invalid)?;
    let store = ctx.store.clone();
    let record_project = id.clone();
    run_op_unit(&ctx, &id, "work_copy_create", move |ops, path| {
        let branch = ops.create_work_copy(path)?;
        // 表示名の記録は操作の成否に影響させない。失敗しても一覧は「無題の案」で出る
        let created_at = created_at_of_work_copy(&branch).unwrap_or_else(now_rfc3339);
        if let Ok(g) = store.lock() {
            let _ = g.add_work_copy(&record_project, &branch, &name, &created_at);
        }
        Ok(())
    })
    .await
}

/// 作業コピーへ切り替える。
#[tauri::command]
#[specta::specta]
pub(crate) async fn switch_work_copy(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    branch: String,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    ensure_enabled(&ctx.store, &id)?;
    run_op_unit(&ctx, &id, "work_copy_switch", move |ops, path| {
        ops.switch_work_copy(path, &branch)
    })
    .await
}

/// 作業コピーを元の作業コピーに取り込む（採用）。ぶつかりは取り込みと同じ型で返し、解消の画面へ進む。
#[tauri::command]
#[specta::specta]
pub(crate) async fn adopt_work_copy(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    branch: String,
) -> Result<PullResult, AppError> {
    let ctx = OpContext::new(app, &state);
    ensure_enabled(&ctx.store, &id)?;
    let result = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "work_copy_adopt",
            trigger: OpTrigger::Manual,
            target: Some(branch.clone()),
            data_is_safe: "ファイルは安全です",
        },
        move |ops, path| {
            // 元の作業コピーは、いまの一覧の中から決める（直列キューの内側で読む）
            let original = original_of(&ops.list_work_copies(path)?)?;
            ops.adopt_work_copy(path, &branch, &original)
        },
        summarize_pull,
    )
    .await
    .map_err(AppError::from)?;
    Ok(pull_result(result))
}

/// 「しまう」（`archived = true`）と、しまうのをやめる（`false`）。表示名の記録だけを変える（git は呼ばない）。
/// いまの作業コピーの「しまう」は画面が出さない（`archivable`）。一覧は、いまの作業コピーを `archived` でも必ず残す。
#[tauri::command]
#[specta::specta]
pub(crate) async fn archive_work_copy(
    state: tauri::State<'_, AppState>,
    id: String,
    branch: String,
    archived: bool,
) -> Result<(), AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        if !core_git::is_work_copy_name(&branch) {
            return Err(not_found());
        }
        let changed = store
            .lock()
            .map_err(|e| db_error(&e))?
            .set_work_copy_archived(&id, &branch, archived)
            .map_err(|e| db_error(&e))?;
        if changed {
            Ok(())
        } else {
            Err(not_found())
        }
    })
    .await
}

/// 戻り値のない作業コピーの操作を、共通の枠（直列キュー・ジャーナル）で実行する
async fn run_op_unit<B>(
    ctx: &OpContext,
    id: &str,
    operation: &'static str,
    body: B,
) -> Result<(), AppError>
where
    B: FnOnce(&Ops, &std::path::Path) -> Result<(), OpsError> + Send + 'static,
{
    run_op(
        ctx,
        id,
        OpSpec {
            operation,
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        body,
        |_| OpSummary {
            detail: "done".to_string(),
            quiet: false,
        },
    )
    .await
    .map_err(AppError::from)
}

/// データベースのエラー（画面の文言は汎用の 3 要素）
fn db_error(e: &dyn std::fmt::Debug) -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: "ファイルは変更されていません".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}
