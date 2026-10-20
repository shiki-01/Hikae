// 拡張機能の管理（設定の「拡張機能」タブ、設計書 11.9）。
//
// - 登録は起動時だけ行う（`load_extensions`）。有効・無効、取り込み、一覧から外すは次回の起動から反映する
// - 標準の拡張（resources/extensions）は無効にできず、一覧から外すこともできない
// - 利用者の拡張（データフォルダの extensions）は、有効の記録（`extensions` テーブル）が無効のとき登録しない
// - 取り込みは開発者モード（設定 `extension_dev_mode`）のときだけ行う（フォルダと zip の両方）
// - 状態を変える操作は、拡張専用の Mutex（`ExtensionLock`）で 1 件ずつ行う。プロジェクトの直列キューは通さない
// - 拡張の ID・フォルダの名前は、パスの組み立てに使わない（利用者の拡張フォルダを読んで照合する）

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use core_preview::{ExtensionHost, ExtensionSummary, InstallError, InstalledExtension};
use core_store::{ExtensionState, Store};
use serde::{Deserialize, Serialize};

use crate::{run_blocking, AppError, AppState};

/// 拡張の取り込み・有効の切り替え・一覧からの除外を 1 件ずつ行うための Mutex
pub type ExtensionLock = Mutex<()>;

/// 拡張のフォルダの場所
#[derive(Debug, Clone)]
pub struct ExtensionDirs {
    /// 同梱の拡張（resources/extensions）。リソースフォルダが分からないときは None
    pub standard: Option<PathBuf>,
    /// 利用者が追加した拡張（アプリのデータフォルダの extensions）
    pub user: PathBuf,
}

/// 一覧の 1 件（画面に出す情報だけ。パスは含めない）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionView {
    pub id: String,
    pub name: String,
    pub version: String,
    /// 対応する拡張子（`.` を除いた英小文字）。全部の拡張子は `*`
    pub file_types: Vec<String>,
    /// 標準（同梱）の拡張なら true。false は利用者が追加した拡張
    pub builtin: bool,
    /// 有効なら true。標準は常に true（無効にできない）
    pub enabled: bool,
}

/// `_removed`・`_failed`・`_staging` は退避先と展開先のフォルダなので、拡張として数えない
const RESERVED_FOLDERS: [&str; 3] = ["_removed", "_failed", "_staging"];

/// 拡張のフォルダの直下にある、拡張の候補のフォルダ（名前の順。退避先は除く）
fn extension_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !RESERVED_FOLDERS.contains(&name))
        })
        .collect();
    dirs.sort();
    dirs
}

/// 標準の拡張のフォルダ一覧（リソースフォルダが無ければ空）
fn standard_folders(dirs: &ExtensionDirs) -> Vec<PathBuf> {
    dirs.standard
        .as_deref()
        .map(extension_folders)
        .unwrap_or_default()
}

/// 標準の拡張の ID の集合（概要を読めたもの。登録の成否には関係なく、標準の ID として扱う）
fn builtin_ids(dirs: &ExtensionDirs) -> HashSet<String> {
    standard_folders(dirs)
        .iter()
        .filter_map(|dir| core_preview::read_summary(dir).ok())
        .map(|summary| summary.id)
        .collect()
}

/// 無効の記録を持つ拡張の ID の集合。記録を読めないときは空（すべて有効として扱う。起動は止めない）
pub fn disabled_ids(states: &[ExtensionState]) -> HashSet<String> {
    states
        .iter()
        .filter(|state| !state.enabled)
        .map(|state| state.id.clone())
        .collect()
}

/// 拡張ホストを作り、標準の拡張と、有効な利用者の拡張を登録する（設計書 11 章）。
///
/// - 同じ ID が標準にもあれば、標準を優先し、利用者の分は登録しない
/// - `disabled` に ID がある利用者の拡張は登録しない（標準は無効にできない）
/// - 失敗は件数だけをログに出す（パス・内容は出さない）。登録できなかった拡張子は「対応しない形式」になる
pub fn load_extensions(dirs: &ExtensionDirs, disabled: &HashSet<String>) -> ExtensionHost {
    let mut host = ExtensionHost::new();
    if dirs.standard.is_none() {
        eprintln!("拡張の登録をスキップしました（リソースフォルダが不明）");
    }
    let standard = standard_folders(dirs);
    let standard_ids = builtin_ids(dirs);
    let mut failed = 0;
    let mut registered = 0;
    for dir in &standard {
        match host.register_dir(dir) {
            Ok(_) => registered += 1,
            Err(_) => failed += 1,
        }
    }
    let mut skipped_disabled = 0;
    for dir in extension_folders(&dirs.user) {
        let Ok(summary) = core_preview::read_summary(&dir) else {
            failed += 1;
            continue;
        };
        if standard_ids.contains(&summary.id) {
            continue;
        }
        if disabled.contains(&summary.id) {
            skipped_disabled += 1;
            continue;
        }
        match host.register_dir(&dir) {
            Ok(_) => registered += 1,
            Err(_) => failed += 1,
        }
    }
    eprintln!(
        "拡張を登録しました: {registered} 件（失敗 {failed} 件、無効のため未登録 {skipped_disabled} 件）"
    );
    host
}

/// 一覧を作る。標準の拡張を先に、利用者の拡張を名前の順で並べる（標準と同じ ID は出さない）
fn collect_views(dirs: &ExtensionDirs, states: &[ExtensionState]) -> Vec<ExtensionView> {
    let mut views = Vec::new();
    for dir in standard_folders(dirs) {
        if let Ok(summary) = core_preview::read_summary(&dir) {
            views.push(view(summary, true, true));
        }
    }
    let standard_ids = builtin_ids(dirs);
    for dir in extension_folders(&dirs.user) {
        let Ok(summary) = core_preview::read_summary(&dir) else {
            continue;
        };
        if standard_ids.contains(&summary.id) {
            continue;
        }
        let enabled = states
            .iter()
            .find(|state| state.id == summary.id)
            .is_none_or(|state| state.enabled);
        views.push(view(summary, false, enabled));
    }
    views
}

fn view(summary: ExtensionSummary, builtin: bool, enabled: bool) -> ExtensionView {
    ExtensionView {
        id: summary.id,
        name: summary.name,
        version: summary.version,
        file_types: summary.file_types,
        builtin,
        enabled,
    }
}

/// 利用者の拡張のうち、ID が一致するものの概要。フォルダを読んで照合する（ID からパスを作らない）
fn find_user_extension(dirs: &ExtensionDirs, id: &str) -> Result<ExtensionSummary, AppError> {
    extension_folders(&dirs.user)
        .iter()
        .filter_map(|dir| core_preview::read_summary(dir).ok())
        .find(|summary| summary.id == id)
        .ok_or_else(not_found_error)
}

/// 標準の拡張への操作（無効化・一覧から外す）を拒否する
fn reject_builtin(dirs: &ExtensionDirs, id: &str) -> Result<(), AppError> {
    if builtin_ids(dirs).contains(id) {
        return Err(AppError {
            code: "extension_builtin".to_string(),
            params: Vec::new(),
            what_happened: "標準の拡張機能は変更できません。".to_string(),
            data_is_safe: "拡張機能は変更されていません。".to_string(),
            next_action: "標準の拡張機能は、無効にしたり一覧から外したりできません".to_string(),
            technical_info: None,
        });
    }
    Ok(())
}

fn not_found_error() -> AppError {
    AppError {
        code: "extension_not_found".to_string(),
        params: Vec::new(),
        what_happened: "拡張機能が見つかりません。".to_string(),
        data_is_safe: "拡張機能は変更されていません。".to_string(),
        next_action: "一覧を開き直して、もう一度選んでください".to_string(),
        technical_info: None,
    }
}

fn lock_error() -> AppError {
    AppError {
        code: "lock_failed".to_string(),
        params: Vec::new(),
        what_happened: "拡張機能の設定を変更できませんでした。".to_string(),
        data_is_safe: "拡張機能は変更されていません。".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some("extension lock".to_string()),
    }
}

fn db_error(e: &dyn std::fmt::Debug) -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "拡張機能の記録を保存できませんでした。".to_string(),
        data_is_safe: "拡張機能のフォルダは変更されていません。".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}

/// 取り込みの失敗を画面の 3 要素の文言にする（パス・内容は含めない）
fn install_error(e: InstallError) -> AppError {
    let (code, what_happened, data_is_safe, next_action) = match e {
        InstallError::SourceUnreadable => (
            "extension_source_unreadable",
            "選んだフォルダを読めませんでした。",
            "拡張機能は変更されていません。",
            "フォルダを選び直してください",
        ),
        InstallError::Manifest(_) => (
            "extension_invalid",
            "拡張機能として正しくありません（拡張機能の設定ファイルか、ファイルの宣言に誤りがあります）。",
            "拡張機能は追加されていません。",
            "拡張機能の作者に確認してください",
        ),
        InstallError::BuiltinId => (
            "extension_builtin_id",
            "同じ ID の標準の拡張機能があるため、追加できません。",
            "拡張機能は追加されていません。",
            "拡張機能の ID を変えてから、もう一度選んでください",
        ),
        InstallError::DuplicateId => (
            "extension_duplicate",
            "同じ ID の拡張機能がすでに追加されています。",
            "追加済みの拡張機能は変更されていません。",
            "追加済みのものを一覧から外してから、もう一度選んでください",
        ),
        InstallError::TooLarge => (
            "extension_too_large",
            "フォルダが大きすぎます（64 MiB を超えるか、ファイルが 2,000 件を超えています）。",
            "拡張機能は追加されていません。",
            "不要なファイルを取り除いてから、もう一度選んでください",
        ),
        InstallError::CopyFailed => (
            "extension_copy_failed",
            "拡張機能をコピーできませんでした。",
            "元のフォルダは変更されていません。",
            "空き容量と権限を確かめて、もう一度試してください",
        ),
        InstallError::MoveFailed => (
            "extension_move_failed",
            "拡張機能を一覧から外せませんでした。",
            "拡張機能のフォルダは変更されていません。",
            "もう一度試してください",
        ),
        InstallError::NotFound => return not_found_error(),
        InstallError::InvalidId => (
            "extension_invalid_id",
            "拡張機能の ID が正しくありません。",
            "拡張機能は変更されていません。",
            "拡張機能を選び直してください",
        ),
        InstallError::ArchiveUnreadable => (
            "extension_archive_unreadable",
            "zip ファイルを開けませんでした（壊れているか、暗号化されているか、対応しない形式です）。",
            "拡張機能は追加されていません。",
            "zip ファイルを確かめてから、もう一度選んでください",
        ),
        InstallError::ArchiveUnsafe => (
            "extension_archive_unsafe",
            "zip の中に安全でない内容があったため、追加しませんでした（危険なファイル名、リンク、極端に圧縮されたファイルのどれかです）。",
            "拡張機能は追加されていません。",
            "拡張機能の作者に確認してください",
        ),
        InstallError::ExtractFailed => (
            "extension_extract_failed",
            "zip を展開できませんでした。",
            "元の zip ファイルは変更されていません。",
            "空き容量と権限を確かめて、もう一度試してください",
        ),
    };
    AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: what_happened.to_string(),
        data_is_safe: data_is_safe.to_string(),
        next_action: next_action.to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}

/// 開発者モードがオフなら拒否する（取り込みの前提。`what` は「フォルダから」などの説明）
fn ensure_dev_mode(store: &Mutex<Store>, what: &str) -> Result<(), AppError> {
    let dev_mode = store
        .lock()
        .map_err(|_| lock_error())?
        .get_settings()
        .map_err(|e| db_error(&e))?
        .extension_dev_mode;
    if dev_mode {
        return Ok(());
    }
    Err(AppError {
        code: "extension_dev_mode_off".to_string(),
        params: Vec::new(),
        what_happened: format!("開発者モードがオフのため、{what}追加できません。"),
        data_is_safe: "拡張機能は追加されていません。".to_string(),
        next_action: "設定の「拡張機能」で開発者モードをオンにしてください".to_string(),
        technical_info: None,
    })
}

/// 取り込んだ拡張を有効として記録し、一覧の 1 件を返す（反映は次回の起動から）
fn record_installed(
    dirs: &ExtensionDirs,
    store: &Mutex<Store>,
    installed: InstalledExtension,
) -> Result<ExtensionView, AppError> {
    let guard = store.lock().map_err(|_| lock_error())?;
    guard
        .set_extension_enabled(&installed.id, &installed.version, true)
        .map_err(|e| db_error(&e))?;
    drop(guard);
    let summary = find_user_extension(dirs, &installed.id)?;
    Ok(view(summary, false, true))
}

/// 拡張の状態を変える操作の直列化（同時に 1 件）
fn lock_ops(lock: &ExtensionLock) -> Result<MutexGuard<'_, ()>, AppError> {
    lock.lock().map_err(|_| lock_error())
}

/// 拡張機能の一覧（標準と、利用者の追加分。無効のものを含む）。読み取りのみ
#[tauri::command]
#[specta::specta]
pub async fn list_extensions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ExtensionView>, AppError> {
    let dirs = state.extension_dirs.clone();
    let store = state.store_clone();
    run_blocking(move || {
        let states = store
            .lock()
            .map_err(|_| lock_error())?
            .list_extension_states()
            .map_err(|e| db_error(&e))?;
        Ok(collect_views(&dirs, &states))
    })
    .await
}

/// 利用者の拡張の有効・無効を記録する。反映は次回の起動から。標準の拡張は拒否する
#[tauri::command]
#[specta::specta]
pub async fn set_extension_enabled(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), AppError> {
    let dirs = state.extension_dirs.clone();
    let store = state.store_clone();
    let lock = state.extension_lock.clone();
    run_blocking(move || {
        let _guard = lock_ops(&lock)?;
        reject_builtin(&dirs, &id)?;
        let summary = find_user_extension(&dirs, &id)?;
        let guard = store.lock().map_err(|_| lock_error())?;
        guard
            .set_extension_enabled(&summary.id, &summary.version, enabled)
            .map_err(|e| db_error(&e))
    })
    .await
}

/// フォルダから拡張を追加する（開発者モードのときだけ）。元のフォルダは変更しない。
/// 反映は次回の起動から。追加した拡張の概要を返す
#[tauri::command]
#[specta::specta]
pub async fn install_extension_from_folder(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<ExtensionView, AppError> {
    let dirs = state.extension_dirs.clone();
    let store = state.store_clone();
    let lock = state.extension_lock.clone();
    run_blocking(move || {
        let _guard = lock_ops(&lock)?;
        ensure_dev_mode(&store, "フォルダから")?;
        let builtin: Vec<String> = builtin_ids(&dirs).into_iter().collect();
        let now = core_store::now_rfc3339();
        let installed =
            core_preview::install_from_folder(Path::new(&path), &dirs.user, &builtin, &now)
                .map_err(install_error)?;
        record_installed(&dirs, &store, installed)
    })
    .await
}

/// zip から拡張を追加する（開発者モードのときだけ）。zip は展開して検査してから取り込む（設計書 11.9）。
/// 元の zip は変更しない。反映は次回の起動から。追加した拡張の概要を返す
#[tauri::command]
#[specta::specta]
pub async fn install_extension_from_zip(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<ExtensionView, AppError> {
    let dirs = state.extension_dirs.clone();
    let store = state.store_clone();
    let lock = state.extension_lock.clone();
    run_blocking(move || {
        let _guard = lock_ops(&lock)?;
        ensure_dev_mode(&store, "zip から")?;
        let builtin: Vec<String> = builtin_ids(&dirs).into_iter().collect();
        let now = core_store::now_rfc3339();
        let installed =
            core_preview::install_from_zip(Path::new(&path), &dirs.user, &builtin, &now)
                .map_err(install_error)?;
        record_installed(&dirs, &store, installed)
    })
    .await
}

/// 利用者の拡張を一覧から外す。フォルダは削除せず `_removed` へ移し、記録を消す。反映は次回の起動から
#[tauri::command]
#[specta::specta]
pub async fn remove_extension(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let dirs = state.extension_dirs.clone();
    let store = state.store_clone();
    let lock = state.extension_lock.clone();
    run_blocking(move || {
        let _guard = lock_ops(&lock)?;
        reject_builtin(&dirs, &id)?;
        let summary = find_user_extension(&dirs, &id)?;
        let now = core_store::now_rfc3339();
        core_preview::move_to_removed(&dirs.user, &summary.id, &now).map_err(install_error)?;
        let guard = store.lock().map_err(|_| lock_error())?;
        guard
            .remove_extension_state(&summary.id)
            .map_err(|e| db_error(&e))
    })
    .await
}
