// 途中で止まった操作（設計書 6.3、E15）の、起動時の自動確定と、利用者による「このまま続ける」。
//
// - 起動時に、中断の行のうち「何も変わっていないと確かめられたもの」だけを自動で閉じる（ダイアログは出さない）。
//   判定の中身は core-ops の `InterruptedCheck::no_change_proven`。閉じるのはジャーナルの行だけ
// - 自動で閉じた件数は、画面の準備ができたあとに一度だけ取り出して知らせる（起動の時点では画面が無い）
// - 「このまま続ける」は直列キューの内側で中断の印を閉じる。作業フォルダ・インデックス・履歴には触れない

use core_ops::is_recoverable_operation;
use tauri_specta::Event;

use crate::events::StatusChanged;
use crate::{run_exclusive, AppError, AppState};

/// 起動時に、何も変わっていないと確かめて自動で閉じたときの理由コード（設計書 6.3）
pub(crate) const CODE_AUTO_CLOSED: &str = "interrupted-no-change";
/// 利用者が「このまま続ける」で閉じたときの理由コード（設計書 6.3）
pub(crate) const CODE_DISMISSED: &str = "interrupted-dismissed";

/// 起動時の確認。中断の行のうち、保存・取り込み系（復旧の対象）で、
/// 作業フォルダ・インデックス・履歴が開始時のまま、と確かめられたものだけを自動で閉じる。
/// 閉じた操作の名前（ジャーナルの操作名）を返す。
///
/// 起動の最初に、スケジューラの自動実行より前に呼ぶ（自動実行の復元点が判定を変えないため）。
/// git を起動できない・読めない行は閉じず、そのまま残す（次の起動で再び判定する）。
pub(crate) fn close_unchanged_at_startup(state: &AppState) -> Vec<String> {
    let rows = match state
        .store
        .lock()
        .ok()
        .and_then(|g| g.list_interrupted().ok())
    {
        Some(rows) => rows,
        None => return Vec::new(),
    };
    let mut closed = Vec::new();
    for row in rows {
        if !is_recoverable_operation(&row.operation) {
            continue;
        }
        // 開始時の HEAD を記録できていない行（この機能より前の行を含む）は、自動では閉じない
        let (Some(started), Some(head)) = (row.started_at_unix(), row.head_before.clone()) else {
            continue;
        };
        let project = match state
            .store
            .lock()
            .ok()
            .and_then(|g| g.get_project(&row.project_id).ok())
        {
            Some(project) => project,
            None => continue,
        };
        // 同一プロジェクトの状態変更と同じ直列実行の内側で判定と確定を行う（不変条件 7）
        let outcome = state
            .locks
            .run(&row.project_id, || -> Result<bool, String> {
                let ops = core_ops::Ops::new(crate::git_runner());
                let check = ops
                    .check_interrupted(&project.path, started, Some(&head))
                    .map_err(|e| format!("{e:?}"))?;
                if !check.no_change_proven() {
                    return Ok(false);
                }
                let guard = state.store.lock().map_err(|e| e.to_string())?;
                guard
                    .close_interrupted(row.id, CODE_AUTO_CLOSED)
                    .map_err(|e| format!("{e:?}"))
            });
        if let Ok(Ok(true)) = outcome {
            closed.push(row.operation.clone());
        }
    }
    closed
}

/// 利用者が「このまま続ける」を選んだ。プロジェクトの中断の印を閉じる。
/// ファイル・インデックス・履歴は変えない。閉じた理由は `interrupted-dismissed` として記録する。
#[tauri::command]
#[specta::specta]
pub(crate) async fn dismiss_interrupted(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let store = state.store_clone();
    let locks = state.locks_clone();
    let project_id = id.clone();
    run_exclusive(
        locks,
        id.clone(),
        "ファイルは変更されていません",
        move || {
            let guard = store.lock().map_err(|e| db_error(&e))?;
            guard
                .resolve_interrupted(&project_id, Some(CODE_DISMISSED))
                .map_err(|e| db_error(&e))?;
            Ok(())
        },
    )
    .await?;
    let _ = StatusChanged { project_id: id }.emit(&app);
    Ok(())
}

/// 起動時に自動で閉じた中断の操作名を、一度だけ取り出す（画面の準備ができたあとに呼ぶ）。
/// 取り出したら空にするため、トーストは一度しか出ない。
#[tauri::command]
#[specta::specta]
pub(crate) fn take_closed_interrupted_notices(state: tauri::State<'_, AppState>) -> Vec<String> {
    state
        .closed_interrupted
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
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
