// 設定のコマンド（設計書 7章・7.1）。値の検証・保存・上書きの合成は core-store が行う。
// ここは AppError への変換と、スケジューラへの反映通知だけを担当する。

use core_store::{AppSettings, SettingsPatch, StoreError};
use serde::{Deserialize, Serialize};

use crate::{run_blocking, AppError, AppState};

/// 画面に返す設定
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SettingsView {
    /// 実際に使う設定。プロジェクトを指定した場合は、全体設定にそのプロジェクトの上書きを重ねたもの
    pub settings: AppSettings,
    /// プロジェクトを指定した場合に、そのプロジェクトで上書きしている項目のキー名
    pub overridden: Vec<String>,
}

fn settings_error(e: StoreError) -> AppError {
    match e {
        StoreError::InvalidData(detail) => AppError {
            what_happened: "設定の値が正しくありません。".to_string(),
            data_is_safe: "設定は変更されていません。".to_string(),
            next_action: "選択肢から選び直してください".to_string(),
            technical_info: Some(detail),
        },
        StoreError::ProjectNotFound(id) => AppError {
            what_happened: "プロジェクトが見つかりません。".to_string(),
            data_is_safe: "設定は変更されていません。".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(id),
        },
        other => AppError {
            what_happened: "設定を読み書きできませんでした。".to_string(),
            data_is_safe: "ファイルは安全です。設定は変更されていない可能性があります。"
                .to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(format!("{other:?}")),
        },
    }
}

fn lock_error() -> AppError {
    AppError {
        what_happened: "設定を読み書きできませんでした。".to_string(),
        data_is_safe: "ファイルは安全です。".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some("lock".to_string()),
    }
}

/// 設定を取得する。`project_id` を指定すると、そのプロジェクトの上書きを反映した設定を返す。
#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    state: tauri::State<'_, AppState>,
    project_id: Option<String>,
) -> Result<SettingsView, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let guard = store.lock().map_err(|_| lock_error())?;
        view(&guard, project_id.as_deref())
    })
    .await
}

/// 設定を更新する。指定した項目だけを書き換え、更新後の設定を返す。
/// `project_id` を指定するとそのプロジェクトの上書きとして保存する（上書きできない項目は拒否）。
#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    state: tauri::State<'_, AppState>,
    project_id: Option<String>,
    patch: SettingsPatch,
) -> Result<SettingsView, AppError> {
    let store = state.store_clone();
    let result = run_blocking(move || {
        let guard = store.lock().map_err(|_| lock_error())?;
        match project_id.as_deref() {
            Some(id) => {
                guard
                    .update_project_settings(id, &patch)
                    .map_err(settings_error)?;
            }
            None => {
                guard.update_settings(&patch).map_err(settings_error)?;
            }
        }
        view(&guard, project_id.as_deref())
    })
    .await?;
    // 取り込み間隔などの変更を、次の巡回を待たずに計画へ反映する
    state.scheduler.refresh();
    Ok(result)
}

/// 初回設定の完了を記録する。
#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let guard = store.lock().map_err(|_| lock_error())?;
        guard
            .update_settings(&SettingsPatch {
                onboarded: Some(true),
                ..SettingsPatch::default()
            })
            .map(|_| ())
            .map_err(settings_error)
    })
    .await
}

fn view(store: &core_store::Store, project_id: Option<&str>) -> Result<SettingsView, AppError> {
    match project_id {
        Some(id) => Ok(SettingsView {
            settings: store.effective_settings(id).map_err(settings_error)?,
            overridden: store.project_overrides(id).map_err(settings_error)?.keys(),
        }),
        None => Ok(SettingsView {
            settings: store.get_settings().map_err(settings_error)?,
            overridden: Vec::new(),
        }),
    }
}
