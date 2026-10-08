// Hikae の Tauri 層。コマンド・イベント、async操作キュー、状態管理を行う。
// `AppError` は Tauri のコマンドが画面へ返す型（コード・params・3 要素の文言を持つ）で、
// Box 化するとコマンドの戻り値型と生成される型定義が変わる。エラー経路は低頻度のため大きさは許容する。
#![allow(clippy::result_large_err)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;
use tauri_specta::{collect_commands, collect_events, Builder, Event};

use core_git::GitRunner;
use core_ops::{Ops, OpsError, SaveOptions};
use core_store::{Project, ProjectLocks, Store};
use core_watch::SyncTask;

mod app_settings;
mod events;
mod github;
mod maintenance;
mod ops_runner;
mod remote;
mod scheduler;

use events::{OpTrigger, StatusChanged};
use ops_runner::{
    memo_labels, pull_outcome_name, push_outcome_name, run_op, size_limits_for, summarize_pull,
    summarize_push, OpContext, OpSpec, OpSummary,
};
use remote::{AddProjectResult, RemoteConnectResult, RemoteRequest};
use scheduler::{pull_task_result, push_task_result, SchedulerHandle};

// ========== エラー型（specta::Type 実装、設計書5章） ==========

/// 何が起きたか、データは無事か、次の行動を含むエラー型。
///
/// 画面は `code` と `params` から言語リソース（i18n キー）で文言を組み立てる。
/// `what_happened` / `data_is_safe` / `next_action`（日本語）は、対応する文言が無い場合の
/// フォールバックとして残す。`params` にはファイル名などの表示用の値だけを入れ、
/// トークンや認証情報、git の標準エラー出力（`technical_info` に入れる）は入れない。
///
/// コード一覧（`OpsError` / 認証エラーと 1 対 1）:
///
/// - 操作: `conflict`（params: `count`）、`git_failed`、`git_timeout`、`safety_check_failed`、
///   `io_error`、`file_in_use`（params: `file`。特定できたときのみ）、`invalid_input`、
///   `restore_point_not_found`、`unexpected`
/// - ファイルを開く: `file_not_found`、`file_unreadable`、`outside_project`
/// - 認証・GitHub: `not_logged_in`（E01）、`github_forbidden`（E02）、`github_rate_limited`、
///   `network_unavailable`（E03）、`github_unavailable`（E04）、`login_not_configured`、
///   `keychain_error`、`login_not_in_progress`、`login_page_unexpected`、`browser_open_failed`、
///   `github_error`
/// - 保存先の作成・接続: `remote_name_taken`（同名のリポジトリが既にある）、`remote_name_invalid`、
///   `remote_owner_invalid`、`remote_public_not_confirmed`、`remote_already_connected`、
///   `project_folder_missing`、`remote_conflict`（`origin` がすでに別の場所を指している。作成の前の
///   検査）、`remote_not_writable`（`.git/config` に書き込めない。作成の前の検査）、
///   `remote_orphaned`（GitHub 上に空の保存先ができたが、プロジェクトに接続できなかった。params:
///   `name` = 作成済みの `owner/name`）
///   （権限不足は `github_forbidden`（E02）、通信できないは `network_unavailable`（E03））
/// - 取得（clone）: `clone_invalid_repo`、`clone_invalid_destination`、`destination_not_empty`、
///   `destination_not_a_folder`、`destination_unreadable`、`remote_not_found`（E16）、
///   `clone_failed`、`clone_timeout`、`clone_register_failed`
/// - プロジェクト: `project_not_found`、`project_already_registered`、`folder_already_registered`、
///   `project_list_failed`、`project_register_failed`、`project_remove_failed`、
///   `relocate_not_a_project`、`relocate_different_project`、`relocate_cannot_verify`、
///   `relocate_failed`
/// - 内部: `database_error`、`task_failed`、`lock_failed`、`settings_io_failed`、
///   `invalid_settings`、`status_failed`、`history_failed`、`diff_failed`、
///   `change_list_failed`、`conflict_list_failed`、`preview_failed`、`open_failed`
///
/// 大きいファイル（E07 / E08）はエラーではなく構造化した結果（`SizeCheckResult`）で返すため、
/// ここにはコードを置かない。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AppError {
    /// 機械可読なエラーコード（上の一覧）。画面の文言の選択に使う
    pub code: String,
    /// 文言に差し込む値（キーと値の組。ファイル名など）
    pub params: Vec<(String, String)>,
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
        let mut params: Vec<(String, String)> = Vec::new();
        let (code, what_happened, data_is_safe, next_action, technical_info) = match e {
            OpsError::Conflict(files) => {
                params.push(("count".to_string(), files.len().to_string()));
                (
                    "conflict",
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
                )
            }
            OpsError::Git(e) => (
                if matches!(e, core_git::GitError::Timeout { .. }) {
                    "git_timeout"
                } else {
                    "git_failed"
                },
                "Git 操作に失敗しました".to_string(),
                "このパソコンのファイルは安全に残っています。".to_string(),
                "もう一度試すか、詳細を確認してください".to_string(),
                Some(format!("{:?}", e)),
            ),
            OpsError::Safety(e) => (
                "safety_check_failed",
                "安全性チェックに失敗しました".to_string(),
                "ファイルは変更されていません。".to_string(),
                "詳細を確認してください".to_string(),
                Some(format!("{:?}", e)),
            ),
            OpsError::Io(e) => (
                "io_error",
                "ファイルアクセスエラーが発生しました".to_string(),
                "ディスク容量や権限を確認してください".to_string(),
                "詳細を確認してください".to_string(),
                Some(e.to_string()),
            ),
            // 設計書 5章 E12: ファイルが他のアプリで使用中（Windows の共有違反）
            OpsError::FileInUse { file } => {
                if let Some(name) = &file {
                    params.push(("file".to_string(), name.clone()));
                }
                (
                "file_in_use",
                match file {
                    Some(name) => {
                        format!(
                            "「{name}」が他のアプリで開かれているため、操作を完了できませんでした"
                        )
                    }
                    None => {
                        "他のアプリで開かれているファイルがあるため、操作を完了できませんでした"
                            .to_string()
                    }
                },
                "ファイルは失われていません。".to_string(),
                "そのファイルを開いているアプリ（Word など）を閉じてから、もう一度お試しください"
                    .to_string(),
                Some("file-in-use".to_string()),
                )
            }
            OpsError::InvalidInput(msg) => (
                "invalid_input",
                "指定された場所またはファイルを扱えませんでした".to_string(),
                "ファイルは変更されていません。".to_string(),
                "選び直してからもう一度試してください".to_string(),
                Some(msg),
            ),
            OpsError::RestorePointNotFound => (
                "restore_point_not_found",
                "中断された操作の前の状態が見つかりませんでした".to_string(),
                "ファイルは変更されていません。データは無事です。".to_string(),
                "取り込み直すか、履歴から元に戻したい時点を選んでください".to_string(),
                None,
            ),
            OpsError::Unexpected(msg) => (
                "unexpected",
                "予期しないエラーが発生しました".to_string(),
                "データは安全に保存されています".to_string(),
                "サポートに連絡してください".to_string(),
                Some(msg),
            ),
        };

        AppError {
            code: code.to_string(),
            params,
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
    /// 進行中の GitHub ログイン（device_code はここにだけ保持し、画面へは渡さない）
    pub login: Arc<core_github::LoginCoordinator>,
    /// ログイン中のユーザー情報の控え（トークンは含まない）
    pub session_user: Arc<Mutex<Option<core_github::User>>>,
}

impl AppState {
    /// 新規作成
    pub fn new(store: Store) -> Self {
        AppState {
            store: Arc::new(Mutex::new(store)),
            locks: Arc::new(ProjectLocks::new()),
            scheduler: SchedulerHandle::new(),
            login: Arc::new(core_github::LoginCoordinator::new()),
            session_user: Arc::new(Mutex::new(None)),
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
            code: "task_failed".to_string(),
            params: Vec::new(),
            what_happened: "タスク実行に失敗しました".to_string(),
            data_is_safe: data_is_safe.to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;
    joined.map_err(|e| AppError {
        code: "lock_failed".to_string(),
        params: Vec::new(),
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
            code: "task_failed".to_string(),
            params: Vec::new(),
            what_happened: "タスク実行に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?
}

/// ID でプロジェクトを取得する（ストアのロックは取得後すぐ解放する）。
fn load_project(store: &Arc<Mutex<Store>>, id: &str) -> Result<Project, AppError> {
    let guard = store.lock().map_err(|e| AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?;
    guard.get_project(id).map_err(|e| AppError {
        code: "project_not_found".to_string(),
        params: Vec::new(),
        what_happened: "プロジェクトが見つかりません".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "プロジェクト一覧から確認してください".to_string(),
        technical_info: Some(format!("{:?}", e)),
    })
}

/// 最初の保存の OID を、未記録のプロジェクトに限って記録する（付け替え時の照合用）。
/// 記録に失敗しても操作の結果には影響させない。
fn record_initial_commit(store: &Arc<Mutex<Store>>, id: &str, path: &std::path::Path, ops: &Ops) {
    let recorded = store
        .lock()
        .ok()
        .and_then(|g| g.get_project(id).ok())
        .is_some_and(|p| p.initial_commit.is_some());
    if recorded {
        return;
    }
    let Ok(Some(oid)) = ops.initial_commit(path) else {
        return;
    };
    if let Ok(guard) = store.lock() {
        let _ = guard.set_initial_commit(id, &oid);
    }
}

/// 登録されたプロジェクトを画面向けの情報にする（読み取りのみ）。
///
/// - フォルダが見つからない・フォルダでない・リポジトリでないときは `folder_missing`（E11）を立て、
///   git を実行しない（フォルダが無いと git がエラーになるため）
/// - 最終アップロード日時は、クラウドに上がっている最新の保存（`@{u}`）の日時。保存先が無い、
///   まだ何も上げていない、取得できないときは None
pub(crate) fn project_info(p: &Project) -> ProjectInfo {
    let folder_missing = core_ops::project_folder_state(&p.path).is_missing();
    let connected = p.remote_url.as_deref().is_some_and(|u| !u.is_empty());
    let last_uploaded_at = if connected && !folder_missing {
        Ops::new(git_runner())
            .last_uploaded_at(&p.path)
            .ok()
            .flatten()
    } else {
        None
    };
    ProjectInfo {
        id: p.id.clone(),
        display_name: p.display_name.clone(),
        path: p.path.clone(),
        remote_url: p.remote_url.clone(),
        owner: p.owner.clone(),
        last_viewed_at: p.last_viewed_at.clone(),
        folder_missing,
        last_uploaded_at,
    }
}

// ========== git の呼び出し口 ==========

/// アプリ共通の GitRunner を作る。
///
/// ネットワーク通信（clone / fetch / push）には、アプリ自身を credential helper として渡す
/// （設計書 8.2）。トークンは URL にもコマンドライン引数にも載せず、git が実行時にこの
/// アプリの `credential` サブコマンドへ問い合わせる。ユーザーのグローバル git 設定には書き込まない。
pub(crate) fn git_runner() -> GitRunner {
    let runner = GitRunner::from_path_env();
    let helper = std::env::current_exe()
        .ok()
        .and_then(|exe| core_github::helper_command(&exe.to_string_lossy()));
    match helper {
        Some(helper) => runner.with_credential_helper(helper),
        None => runner,
    }
}

/// git の credential helper として動く（`hikae credential <get|store|erase>`）。終了コードを返す。
///
/// `get` のときだけ、https://github.com 宛ての要求にトークンを返す。それ以外の要求・ホストには
/// 何も出力しない。トークンは標準出力（git への応答）以外には出さない。
pub fn run_credential_helper(operation: Option<&str>) -> i32 {
    use std::io::{BufRead, Write};

    let operation = operation.unwrap_or("");
    // 要求は「key=value」の行で、空行で終わる
    let mut input = String::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            break;
        }
        input.push_str(&line);
        input.push('\n');
    }
    input.push('\n');

    let token = if operation == "get" {
        core_github::TokenStore::load().ok().flatten()
    } else {
        None
    };
    if let Some(response) = core_github::respond_to_git(operation, &input, token.as_ref()) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(response.as_bytes());
        let _ = out.flush();
    }
    0
}

// ========== Tauri コマンド（全て async/spawn_blocking） ==========

/// プロジェクト一覧を取得。
#[tauri::command]
#[specta::specta]
async fn list_projects(state: tauri::State<'_, AppState>) -> Result<Vec<ProjectInfo>, AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            code: "project_list_failed".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクト一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let projects = store_guard.list_projects().map_err(|e| AppError {
            code: "project_list_failed".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクト一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;
        // git を実行する間はデータベースのロックを持たない
        drop(store_guard);

        Ok(projects.iter().map(project_info).collect())
    })
    .await
    .map_err(|e| AppError {
        code: "task_failed".to_string(),
        params: Vec::new(),
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// プロジェクトを追加（既存フォルダ登録・新規作成）。GitHub から取得する場合は `clone_project`。
///
/// `remote` を渡すと、ローカルの登録に続けて GitHub 上にリポジトリを作り、保存先として接続し、
/// 初回の保存（変更があれば）と初回のアップロードまで行う（設計書 4.6）。途中で失敗しても
/// ローカルの登録は残る（保存先の無いプロジェクトとして使え、あとから `connect_remote` で接続できる）。
/// その場合は `AddProjectResult.remote.error` に 3 要素のエラーが入る。
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn add_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    display_name: String,
    path: PathBuf,
    owner: String,
    remote_url: Option<String>,
    remote: Option<RemoteRequest>,
) -> Result<AddProjectResult, AppError> {
    let store = state.store.clone();
    let locks = state.locks.clone();
    let id_clone = id.clone();
    let id_for_flow = id.clone();
    // 公開の確認が済んでいない要求は、何も作る前に断る
    if let Some(request) = &remote {
        remote::check_public_confirmed(request)?;
    }
    // 保存先を作る場合の所有者を、プロジェクトの所有者として記録する
    let owner = remote.as_ref().map_or(owner, |r| r.owner.clone());
    // 署名は実際のログインユーザー（数値 ID とログイン名）から決める。
    // 未ログイン・オフラインならローカル専用の既定にし、保存時に実ユーザーへ更新する
    let signing = github::signing_user(&state).await;
    let has_origin = run_exclusive(
        locks,
        id,
        "ファイルは変更されていません",
        move || {
            let runner = crate::git_runner();
            let ops = Ops::new(runner);
            let identity =
                core_ops::resolve_identity(signing.as_ref().map(|(i, l)| (*i, l.as_str())));

            ops.init_project(&path, remote_url.as_deref(), &identity)
                .map_err(AppError::from_ops_error)?;

            // 既存のリポジトリがすでに保存先（origin）を持っているときは、それを使う（作り直さない）
            let existing_origin = ops.origin_url(&path).ok().flatten();
            let has_origin = existing_origin.is_some();
            let stored_remote = remote_url.clone().or(existing_origin);

            // 既存のリポジトリを登録する場合に備えて、最初の保存の OID を先に調べておく
            let initial_commit = ops.initial_commit(&path).ok().flatten();
            let store_guard = store.lock().map_err(|e| AppError {
                code: "database_error".to_string(),
                params: Vec::new(),
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
                    stored_remote.as_deref(),
                    &owner,
                    "main",
                )
                .map_err(|e| AppError {
                    code: "project_register_failed".to_string(),
                    params: Vec::new(),
                    what_happened: "プロジェクト登録に失敗しました".to_string(),
                    data_is_safe: "ファイルは変更されていません".to_string(),
                    next_action: "もう一度試してください".to_string(),
                    technical_info: Some(format!("{:?}", e)),
                })?;

            // 最初の保存の OID を記録する（保存先 URL が無いプロジェクトの付け替え照合用）。
            // まだ保存が無い場合は None のままで、最初の保存のときに記録する
            if let Some(oid) = initial_commit {
                let _ = store_guard.set_initial_commit(&id_clone, &oid);
            }

            Ok(has_origin)
        },
    )
    .await?;

    // ローカルの登録はここで完了している。保存先の作成に失敗しても登録は残す。
    // すでに保存先（origin）を持つリポジトリは、それを使うため新しく作らない
    let Some(request) = remote.filter(|_| !has_origin) else {
        return Ok(AddProjectResult { remote: None });
    };
    let result = match remote::connect_flow(app, &state, &id_for_flow, request, true).await {
        Ok(result) => result,
        Err(error) => RemoteConnectResult::not_connected(error),
    };
    Ok(AddProjectResult {
        remote: Some(result),
    })
}

/// プロジェクトを削除（登録のみ。フォルダは消さない）。
#[tauri::command]
#[specta::specta]
async fn remove_project(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store_guard = store.lock().map_err(|e| AppError {
            code: "database_error".to_string(),
            params: Vec::new(),
            what_happened: "データベースアクセスに失敗しました".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        store_guard.remove_project(&id).map_err(|e| AppError {
            code: "project_remove_failed".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクト削除に失敗しました".to_string(),
            data_is_safe: "フォルダ内のファイルは残っています".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        Ok(())
    })
    .await
    .map_err(|e| AppError {
        code: "task_failed".to_string(),
        params: Vec::new(),
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
            code: "status_failed".to_string(),
            params: Vec::new(),
            what_happened: "状態取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            code: "project_not_found".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = crate::git_runner();
        let ops = Ops::new(runner);

        let interrupted_operation = store
            .lock()
            .ok()
            .and_then(|g| g.interrupted_operation(&id).ok().flatten())
            .map(|e| e.operation);

        // フォルダが見つからない（E11）ときは git を実行しない（実行するとエラーになるため）
        if core_ops::project_folder_state(&project.path).is_missing() {
            return Ok(SyncStatus {
                folder_missing: true,
                interrupted_operation,
                ..SyncStatus::default()
            });
        }

        let sync = ops
            .sync_state(&project.path)
            .map_err(AppError::from_ops_error)?;
        let conflicts = ops
            .conflicts(&project.path)
            .map_err(AppError::from_ops_error)?;
        // 件数は変更ファイル一覧と同じ関数から出す（一覧と件数がずれない）
        let unsaved_changes = ops
            .list_changes(&project.path)
            .map_err(AppError::from_ops_error)?
            .len();

        // 保存先は設定済みだが一度もアップロードしていない（upstream なし）ときは、
        // 作った保存のすべてがアップロード待ち
        let connected = project.remote_url.as_deref().is_some_and(|u| !u.is_empty());
        let upload_pending = if sync.has_upstream || !connected {
            sync.ahead
        } else {
            ops.commit_count(&project.path)
                .map_err(AppError::from_ops_error)?
        };

        Ok(SyncStatus {
            unsaved_changes: u32::try_from(unsaved_changes).unwrap_or(u32::MAX),
            upload_pending,
            pull_pending: sync.behind,
            has_conflicts: !conflicts.is_empty(),
            is_syncing: false,
            interrupted_operation,
            folder_missing: false,
        })
    })
    .await
    .map_err(|e| AppError {
        code: "task_failed".to_string(),
        params: Vec::new(),
        what_happened: "タスク実行に失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(e.to_string()),
    })?
}

/// 変更ファイル一覧を取得（読み取りのみ。解析は core-ops）。
#[tauri::command]
#[specta::specta]
async fn list_changes(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<ChangeFile>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(crate::git_runner());
        let changes = ops
            .list_changes(&project.path)
            .map_err(AppError::from_ops_error)?;
        Ok(changes.into_iter().map(ChangeFile::from).collect())
    })
    .await
}

/// 保存（commit）を実行。
///
/// 保存前に新規・変更ファイルのサイズを検査する（設計書 4.1 手順 1）。大きいファイルがあれば
/// 何も保存せず、`size_check` に該当ファイルとサイズを入れて返す（エラーではない）。
/// 画面は E07 / E08 の 2 択を出し、選択を `save_with_size_choice` で送って保存をやり直す。
#[tauri::command]
#[specta::specta]
async fn save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    message: String,
) -> Result<SaveResult, AppError> {
    run_save(app, &state, id, message, SaveSizeChoice::default()).await
}

/// 大きいファイルについての選択（E07 / E08）を添えて保存をやり直す。
#[tauri::command]
#[specta::specta]
async fn save_with_size_choice(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    message: String,
    choice: SaveSizeChoice,
) -> Result<SaveResult, AppError> {
    run_save(app, &state, id, message, choice).await
}

async fn run_save(
    app: tauri::AppHandle,
    state: &AppState,
    id: String,
    message: String,
    choice: SaveSizeChoice,
) -> Result<SaveResult, AppError> {
    let ctx = OpContext::new(app, state);
    let store = state.store_clone();
    let touch_id = id.clone();
    let memo = message.clone();
    // ログイン済みのユーザーが分かっていれば、保存の直前にリポジトリ単位の署名を最新にする。
    // 分からない（未ログイン・オフライン）ときは既存の署名を変えない
    let signing = github::cached_signing_user(state);
    // 大きいファイルの警告閾値（設計書 7章。プロジェクト別の上書きを含む）。読めなければ既定値
    let limits = size_limits_for(&store, &id);
    let options = SaveOptions {
        limits,
        accept_warned: choice.accept_warned,
        exclude: choice.exclude,
    };

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
            if let Some((uid, login)) = &signing {
                let identity = core_ops::resolve_identity(Some((*uid, login.as_str())));
                ops.apply_identity(path, &identity)?;
            }
            let outcome = ops.save_with(path, &memo, &options)?;
            if let Ok(guard) = store.lock() {
                let _ = guard.touch_project(&touch_id);
            }
            // 最初の保存だった場合に、付け替え照合用の OID を記録する
            record_initial_commit(&store, &touch_id, path, ops);
            Ok(outcome)
        },
        |o| OpSummary {
            detail: match o {
                core_ops::SaveOutcome::Saved { .. } => "saved",
                core_ops::SaveOutcome::NothingToSave => "nothing-to-save",
                core_ops::SaveOutcome::NeedsSizeDecision(_) => "needs-size-decision",
            }
            .to_string(),
            quiet: false,
        },
    )
    .await?;

    let (commit, size_check) = match outcome {
        core_ops::SaveOutcome::Saved { commit, .. } => (Some(commit), None),
        core_ops::SaveOutcome::NothingToSave => (None, None),
        core_ops::SaveOutcome::NeedsSizeDecision(found) => (None, Some(size_check_result(found))),
    };

    // 「保存時に自動アップロード」がオンなら、スケジューラがキュー経由でアップロードする
    if commit.is_some() {
        state.scheduler.request_push(&id);
    }

    Ok(SaveResult {
        commit,
        message: Some(message),
        size_check,
    })
}

/// 検査結果を画面向けの型にする（サイズはバイト）
fn size_check_result(found: core_ops::SizeFindings) -> SizeCheckResult {
    let item = |f: core_ops::LargeFile| LargeFileItem {
        path: f.path,
        size: f.size as f64,
    };
    SizeCheckResult {
        blocked: found.blocked.into_iter().map(item).collect(),
        warned: found.warned.into_iter().map(item).collect(),
    }
}

/// 未保存の変更から保存メモの案を作る（ルールベース、設計書 10.3）。
/// 読み取りのみのため直列キューは通さない。
#[tauri::command]
#[specta::specta]
async fn suggest_memo(state: tauri::State<'_, AppState>, id: String) -> Result<String, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(crate::git_runner());
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
        let ops = Ops::new(crate::git_runner());
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

/// プロジェクト内のファイルを既定のアプリで開く。
/// 相対パスはここで検証し、プロジェクト外（`..`・絶対パス・シンボリックリンク経由）は開かない。
/// 読み取りのみのため直列キューは通さない。
#[tauri::command]
#[specta::specta]
async fn open_project_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    relative_path: String,
) -> Result<(), AppError> {
    let store = state.store_clone();
    let target = run_blocking(move || {
        let project = load_project(&store, &id)?;
        core_ops::resolve_in_project(&project.path, &relative_path).map_err(open_path_error)
    })
    .await?;

    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| AppError {
            code: "open_failed".to_string(),
            params: Vec::new(),
            what_happened: "ファイルを開けませんでした".to_string(),
            data_is_safe: "ファイルは変更されていません。".to_string(),
            next_action: "対応するアプリがインストールされているか確認してください".to_string(),
            technical_info: Some(e.to_string()),
        })
}

/// パス検証の拒否理由を AppError へ変換する。
fn open_path_error(e: core_ops::OpenPathError) -> AppError {
    use core_ops::OpenPathError as E;
    let (code, what_happened, next_action) = match &e {
        E::NotFound => (
            "file_not_found",
            "開こうとしたファイルが見つかりません",
            "削除や移動がされていないか確認してください",
        ),
        E::RootUnavailable(_) | E::Io(_) => (
            "file_unreadable",
            "ファイルを確認できませんでした",
            "フォルダの場所や権限を確認してください",
        ),
        E::Invalid | E::NotRelative | E::Outside => (
            "outside_project",
            "プロジェクトの外にあるファイルは開けません",
            "プロジェクト内のファイルを選んでください",
        ),
    };
    AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: what_happened.to_string(),
        data_is_safe: "ファイルは変更されていません。".to_string(),
        next_action: next_action.to_string(),
        technical_info: Some(e.to_string()),
    }
}

/// 競合ファイルを UI 向けの型へ変換する
fn conflict_item(f: core_ops::ConflictFile) -> ConflictItem {
    ConflictItem {
        path: f.path,
        this_saved_at: f.this_saved_at.map(|t| t as f64),
        cloud_saved_at: f.cloud_saved_at.map(|t| t as f64),
        this_pc_name: f.this_pc_name,
        cloud_pc_name: f.cloud_pc_name,
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
    let limits = size_limits_for(&ctx.store, &id);
    let result = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "pull",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        move |ops, path| ops.pull_with(path, limits),
        summarize_pull,
    )
    .await;
    // 手動の結果もスケジューラの計画に反映する（成功すれば失敗状態が解除される）
    state
        .scheduler
        .note_result(&id, SyncTask::Pull, pull_task_result(&result));
    let outcome = result?;

    let name = pull_outcome_name(&outcome).to_string();
    let (conflicts, size_check) = match outcome {
        core_ops::PullOutcome::Conflicted { files } => {
            (files.into_iter().map(conflict_item).collect(), None)
        }
        core_ops::PullOutcome::NeedsSizeDecision(found) => (vec![], Some(size_check_result(found))),
        _ => (vec![], None),
    };
    Ok(PullResult {
        outcome: name,
        conflicts,
        size_check,
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
    let limits = size_limits_for(&ctx.store, &id);
    let result = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "push",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        move |ops, path| ops.upload_with(path, limits),
        summarize_push,
    )
    .await;
    state
        .scheduler
        .note_result(&id, SyncTask::Push, push_task_result(&result));
    let outcome = result?;

    let size_check = match &outcome {
        core_ops::UploadOutcome::NeedsSizeDecision(found) => Some(size_check_result(found.clone())),
        _ => None,
    };
    Ok(PushResult {
        outcome: push_outcome_name(&outcome).to_string(),
        size_check,
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
            code: "history_failed".to_string(),
            params: Vec::new(),
            what_happened: "履歴取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            code: "project_not_found".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = crate::git_runner();
        let ops = Ops::new(runner);

        // フォルダが見つからない（E11）ときは git を実行せず、空の履歴を返す
        if core_ops::project_folder_state(&project.path).is_missing() {
            return Ok(Vec::new());
        }

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
                pc_name: e.pc_name,
                cloud_synced: e.cloud_synced,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError {
        code: "task_failed".to_string(),
        params: Vec::new(),
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
            code: "diff_failed".to_string(),
            params: Vec::new(),
            what_happened: "差分取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            code: "project_not_found".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = crate::git_runner();
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
        code: "task_failed".to_string(),
        params: Vec::new(),
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
            code: "preview_failed".to_string(),
            params: Vec::new(),
            what_happened: "プレビュー生成に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            code: "project_not_found".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = crate::git_runner();
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
        code: "task_failed".to_string(),
        params: Vec::new(),
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
) -> Result<RestoreAllResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let target = commit.clone();
    let undo_token = run_op(
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
    Ok(RestoreAllResult { undo_token })
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
            code: "conflict_list_failed".to_string(),
            params: Vec::new(),
            what_happened: "競合一覧取得に失敗しました".to_string(),
            data_is_safe: "ファイルは安全です".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;

        let project = store_guard.get_project(&id).map_err(|e| AppError {
            code: "project_not_found".to_string(),
            params: Vec::new(),
            what_happened: "プロジェクトが見つかりません".to_string(),
            data_is_safe: "何も変更されていません".to_string(),
            next_action: "プロジェクト一覧から確認してください".to_string(),
            technical_info: Some(format!("{:?}", e)),
        })?;

        drop(store_guard);

        let runner = crate::git_runner();
        let ops = Ops::new(runner);

        let conflicts = ops
            .conflicts(&project.path)
            .map_err(AppError::from_ops_error)?;

        Ok(conflicts.into_iter().map(conflict_item).collect())
    })
    .await
    .map_err(|e| AppError {
        code: "task_failed".to_string(),
        params: Vec::new(),
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

/// 1 ファイルだけを戻した場合の影響（読み取りのみのため直列キューは通さない）。
#[tauri::command]
#[specta::specta]
async fn restore_file_preview(
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
    path: String,
) -> Result<RestoreFilePreviewData, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(crate::git_runner());
        let p = ops
            .restore_file_preview(&project.path, &commit, &path)
            .map_err(AppError::from_ops_error)?;
        Ok(RestoreFilePreviewData {
            kind: match p.kind {
                core_ops::RestoreFileKind::Overwrite => RestoreFileKindData::Overwrite,
                core_ops::RestoreFileKind::Recreate => RestoreFileKindData::Recreate,
                core_ops::RestoreFileKind::Unchanged => RestoreFileKindData::Unchanged,
                core_ops::RestoreFileKind::NotInThatPoint => RestoreFileKindData::NotInThatPoint,
            },
            size_now: p.size_now.map(|s| s as f64),
            size_then: p.size_then.map(|s| s as f64),
        })
    })
    .await
}

/// 指定した保存時点の 1 ファイルだけを戻す。復元点は core-ops が作る。
#[tauri::command]
#[specta::specta]
async fn restore_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
    path: String,
) -> Result<RestoreFileResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let target = format!("{commit}:{path}");
    let outcome = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "restore-file",
            trigger: OpTrigger::Manual,
            target: Some(target),
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, project_path| ops.restore_file(project_path, &commit, &path),
        |o| OpSummary {
            detail: match o {
                core_ops::RestoreFileOutcome::Restored { .. } => "restored",
                core_ops::RestoreFileOutcome::NotInThatPoint => "not-in-that-point",
                core_ops::RestoreFileOutcome::IgnoredFileInTheWay => "ignored-file-in-the-way",
            }
            .to_string(),
            quiet: false,
        },
    )
    .await?;

    Ok(match outcome {
        core_ops::RestoreFileOutcome::Restored { undo_ref } => RestoreFileResult {
            outcome: RestoreFileResultKind::Restored,
            undo_token: undo_ref,
        },
        core_ops::RestoreFileOutcome::NotInThatPoint => RestoreFileResult {
            outcome: RestoreFileResultKind::NotInThatPoint,
            undo_token: None,
        },
        core_ops::RestoreFileOutcome::IgnoredFileInTheWay => RestoreFileResult {
            outcome: RestoreFileResultKind::IgnoredFileInTheWay,
            undo_token: None,
        },
    })
}

/// 元に戻すの取り消し。`restore_point` は復元点の ref 名（`refs/hikae/` 配下）または完全な OID。
#[tauri::command]
#[specta::specta]
async fn undo_restore(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    restore_point: String,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    let target = restore_point.clone();
    run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "undo-restore",
            trigger: OpTrigger::Manual,
            target: Some(target),
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, path| ops.undo_restore(path, &restore_point),
        |_| OpSummary {
            detail: "undone".to_string(),
            quiet: false,
        },
    )
    .await?;
    Ok(())
}

/// 変更のぶつかり解消を中断し、取り込む前の状態に戻す。
#[tauri::command]
#[specta::specta]
async fn abort_merge(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let ctx = OpContext::new(app, &state);
    run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "abort-merge",
            trigger: OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは変更されていません",
        },
        |ops, path| ops.abort_merge(path),
        |_| OpSummary {
            detail: "aborted".to_string(),
            quiet: false,
        },
    )
    .await?;
    Ok(())
}

/// 外部のファイルをプロジェクトへコピーする（上書きせず、保存もしない）。
#[tauri::command]
#[specta::specta]
async fn add_files(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    source_paths: Vec<PathBuf>,
    dest_subdir: String,
) -> Result<AddFilesResult, AppError> {
    let ctx = OpContext::new(app, &state);
    let target = if dest_subdir.is_empty() {
        None
    } else {
        Some(dest_subdir.clone())
    };
    let outcome = run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "add-files",
            trigger: OpTrigger::Manual,
            target,
            data_is_safe: "元のファイルは変更されていません",
        },
        move |ops, path| ops.add_files(path, &source_paths, &dest_subdir),
        |o| OpSummary {
            detail: if o.added.is_empty() {
                "nothing-added"
            } else {
                "added"
            }
            .to_string(),
            quiet: false,
        },
    )
    .await?;

    Ok(AddFilesResult {
        added: outcome
            .added
            .into_iter()
            .map(|f| AddedFileItem {
                path: f.path,
                renamed: f.renamed,
                large: f.large,
            })
            .collect(),
        rejected: outcome
            .rejected
            .into_iter()
            .map(|r| {
                let (reason, size) = match r.reason {
                    core_ops::AddRejectReason::TooLarge { size } => {
                        (AddRejectKind::TooLarge, Some(size as f64))
                    }
                    core_ops::AddRejectReason::NotAFile => (AddRejectKind::NotAFile, None),
                    core_ops::AddRejectReason::Unreadable => (AddRejectKind::Unreadable, None),
                };
                RejectedFileItem {
                    name: r.name,
                    reason,
                    size,
                }
            })
            .collect(),
    })
}

/// 過去の版のファイルを書き出す一時フォルダ（`<temp>/hikae-preview`）。
/// 配下は `<プロジェクト ID>/<短縮コミット>/<相対パス>`。古いものは起動時に削除する。
fn preview_root() -> PathBuf {
    std::env::temp_dir().join("hikae-preview")
}

/// プロジェクト ID を一時フォルダ名に使えるか（パス区切りや `..` を含まない）
fn is_safe_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 過去の版のファイルを開く（設計書 4.7）。
///
/// 指定時点の内容をアプリ専用の一時フォルダへ書き出して読み取り専用にし、既定のアプリで開く。
/// プロジェクトのファイルとリポジトリは変更しない（読み取りのみのため直列キューは通さない）。
/// パスは `..`・絶対パス・`.git` 配下を拒否する。
#[tauri::command]
#[specta::specta]
async fn open_file_at(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
    relative_path: String,
) -> Result<(), AppError> {
    let store = state.store_clone();
    let target = run_blocking(move || {
        let project = load_project(&store, &id)?;
        if !is_safe_folder_name(&id) {
            return Err(AppError {
                code: "project_not_found".to_string(),
                params: Vec::new(),
                what_happened: "プロジェクトが見つかりません".to_string(),
                data_is_safe: "何も変更されていません".to_string(),
                next_action: "プロジェクト一覧から確認してください".to_string(),
                technical_info: None,
            });
        }
        let ops = Ops::new(crate::git_runner());
        ops.export_file_at(
            &project.path,
            &commit,
            &relative_path,
            &preview_root().join(&id),
        )
        .map_err(AppError::from_ops_error)
    })
    .await?;

    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| AppError {
            code: "open_failed".to_string(),
            params: Vec::new(),
            what_happened: "ファイルを開けませんでした".to_string(),
            data_is_safe: "ファイルは変更されていません。".to_string(),
            next_action: "対応するアプリがインストールされているか確認してください".to_string(),
            technical_info: Some(e.to_string()),
        })
}

/// 中断された操作（E15）の直前の復元点へ、作業フォルダを戻す。
///
/// 対象は保存・取り込み系（`save` / `pull` / `push` / `resolve`）。復元点は
/// `refs/hikae/backup/` の、その操作が始まった後に作られた最新のもの。戻す前にいまの状態の
/// 復元点を作り、`restore --source` で戻す（`reset --hard` / `checkout -f` は使わない）。
/// 実行は直列キューとジャーナルを通す。復元点が見つからなければ何も変更せずエラーを返し、
/// 中断の印は残る。成功すると中断の印を解除する。
#[tauri::command]
#[specta::specta]
async fn recover_interrupted(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let store = state.store_clone();
    let lookup_id = id.clone();
    let entry = run_blocking(move || {
        let guard = store.lock().map_err(|e| AppError {
            code: "database_error".to_string(),
            params: Vec::new(),
            what_happened: "データベースアクセスに失敗しました".to_string(),
            data_is_safe: "ファイルは変更されていません".to_string(),
            next_action: "もう一度試してください".to_string(),
            technical_info: Some(e.to_string()),
        })?;
        guard
            .interrupted_operation(&lookup_id)
            .map_err(|e| AppError {
                code: "database_error".to_string(),
                params: Vec::new(),
                what_happened: "データベースアクセスに失敗しました".to_string(),
                data_is_safe: "ファイルは変更されていません".to_string(),
                next_action: "もう一度試してください".to_string(),
                technical_info: Some(format!("{e:?}")),
            })
    })
    .await?;

    let Some(entry) = entry else {
        return Err(AppError {
            code: "no_interrupted_operation".to_string(),
            params: Vec::new(),
            what_happened: "中断された操作は見つかりませんでした".to_string(),
            data_is_safe: "ファイルは変更されていません。".to_string(),
            next_action: "画面を更新してください".to_string(),
            technical_info: None,
        });
    };
    if !core_ops::is_recoverable_operation(&entry.operation) {
        return Err(AppError {
            code: "not_recoverable".to_string(),
            params: vec![("operation".to_string(), entry.operation)],
            what_happened: "中断された操作は、自動では元に戻せません".to_string(),
            data_is_safe: "ファイルは変更されていません。".to_string(),
            next_action: "履歴から、戻したい時点を選んでください".to_string(),
            technical_info: None,
        });
    }
    // 開始時刻が読めなければ復元点を特定できない（古い復元点を使わない）
    let Some(started_at) = entry.started_at_unix() else {
        return Err(AppError::from_ops_error(OpsError::RestorePointNotFound));
    };

    let ctx = OpContext::new(app, &state);
    let operation = entry.operation.clone();
    let target = entry.operation;
    run_op(
        &ctx,
        &id,
        OpSpec {
            operation: "recover",
            trigger: OpTrigger::Manual,
            target: Some(target),
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, path| ops.recover_interrupted(path, &operation, started_at),
        |_| OpSummary {
            detail: "recovered".to_string(),
            quiet: false,
        },
    )
    .await?;
    Ok(())
}

/// フォルダが見つからないプロジェクトの登録パスを付け替える。
/// 付け替え先が同じプロジェクトと確認できた場合のみ更新し、ファイルは一切変更しない。
#[tauri::command]
#[specta::specta]
async fn relocate_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    new_path: PathBuf,
) -> Result<ProjectInfo, AppError> {
    let store = state.store_clone();
    let locks = state.locks_clone();
    let info = run_exclusive(
        locks,
        id.clone(),
        "ファイルは変更されていません",
        move || {
            let project = load_project(&store, &id)?;
            let ops = Ops::new(crate::git_runner());
            let check = ops
                .check_relocation_with(
                    &new_path,
                    project.remote_url.as_deref(),
                    project.initial_commit.as_deref(),
                )
                .map_err(AppError::from_ops_error)?;
            let (code, what_happened, next_action) = match check {
                core_ops::RelocateCheck::Same => ("", "", ""),
                core_ops::RelocateCheck::NotARepository => (
                    "relocate_not_a_project",
                    "選んだフォルダはプロジェクトのフォルダではありません",
                    "プロジェクトのフォルダそのものを選び直してください",
                ),
                core_ops::RelocateCheck::DifferentRepository => (
                    "relocate_different_project",
                    "選んだフォルダは、このプロジェクトのものではありません",
                    "移動したプロジェクトのフォルダを選び直してください",
                ),
                core_ops::RelocateCheck::CannotVerify => (
                    "relocate_cannot_verify",
                    "このプロジェクトは保存先も最初の保存の記録も登録されていないため、同じフォルダか確認できません",
                    "一覧から外して、フォルダを登録し直してください",
                ),
            };
            if check != core_ops::RelocateCheck::Same {
                return Err(AppError {
                    code: code.to_string(),
                    params: Vec::new(),
                    what_happened: what_happened.to_string(),
                    data_is_safe: "ファイルは変更されていません。".to_string(),
                    next_action: next_action.to_string(),
                    technical_info: Some(format!("{check:?}")),
                });
            }

            let guard = store.lock().map_err(|e| AppError {
                code: "database_error".to_string(),
                params: Vec::new(),
                what_happened: "データベースアクセスに失敗しました".to_string(),
                data_is_safe: "ファイルは変更されていません。".to_string(),
                next_action: "もう一度試してください".to_string(),
                technical_info: Some(e.to_string()),
            })?;
            guard
                .update_project_path(&id, &new_path)
                .map_err(|e| AppError {
                    code: "relocate_failed".to_string(),
                    params: Vec::new(),
                    what_happened: "フォルダの場所を更新できませんでした".to_string(),
                    data_is_safe: "ファイルは変更されていません。".to_string(),
                    next_action: "もう一度試してください".to_string(),
                    technical_info: Some(format!("{e:?}")),
                })?;
            let updated = guard.get_project(&id).map_err(|e| AppError {
                code: "project_not_found".to_string(),
                params: Vec::new(),
                what_happened: "プロジェクトが見つかりません".to_string(),
                data_is_safe: "ファイルは変更されていません。".to_string(),
                next_action: "プロジェクト一覧から確認してください".to_string(),
                technical_info: Some(format!("{e:?}")),
            })?;
            drop(guard);
            Ok(project_info(&updated))
        },
    )
    .await?;

    let _ = StatusChanged {
        project_id: info.id.clone(),
    }
    .emit(&app);
    Ok(info)
}

/// 保存時点で変更されたファイルの一覧（変更の種類つき、読み取りのみ）。
#[tauri::command]
#[specta::specta]
async fn list_point_changes(
    state: tauri::State<'_, AppState>,
    id: String,
    commit: String,
) -> Result<Vec<PointChangeItem>, AppError> {
    let store = state.store_clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let ops = Ops::new(crate::git_runner());
        let changes = ops
            .list_point_changes(&project.path, &commit)
            .map_err(AppError::from_ops_error)?;
        Ok(changes
            .into_iter()
            .map(|c| PointChangeItem {
                path: c.path,
                old_path: c.old_path,
                kind: match c.kind {
                    core_ops::PointChangeKind::Added => PointChangeKindData::Added,
                    core_ops::PointChangeKind::Modified => PointChangeKindData::Modified,
                    core_ops::PointChangeKind::Deleted => PointChangeKindData::Deleted,
                    core_ops::PointChangeKind::Renamed => PointChangeKindData::Renamed,
                },
            })
            .collect())
    })
    .await
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
    /// 登録したフォルダが見つからない・フォルダでない・リポジトリでない（E11）。true のとき git は実行していない
    pub folder_missing: bool,
    /// クラウドに上がっている最新の保存の日時（ISO 8601）。保存先が無い、まだ何も上げていない、
    /// フォルダが見つからないときは null
    pub last_uploaded_at: Option<String>,
}

/// 変更ファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChangeFile {
    pub path: String,
    pub kind: ChangeKind,
    /// 変更のぶつかり中のファイルか
    pub conflicted: bool,
}

impl From<core_ops::ChangedFile> for ChangeFile {
    fn from(file: core_ops::ChangedFile) -> Self {
        ChangeFile {
            path: file.path,
            kind: match file.kind {
                core_ops::ChangedKind::Added => ChangeKind::Added,
                core_ops::ChangedKind::Modified => ChangeKind::Modified,
                core_ops::ChangedKind::Deleted => ChangeKind::Deleted,
                core_ops::ChangedKind::Renamed => ChangeKind::Renamed,
            },
            conflicted: file.conflicted,
        }
    }
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
    /// 大きいファイルがあり、保存しなかった場合の内容（E07 / E08）。なければ null
    pub size_check: Option<SizeCheckResult>,
}

/// 保存前のサイズ検査で見つかったファイル（保存はしていない）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SizeCheckResult {
    /// 100MB を超えるため保存できないファイル（E07: 「外して保存」か「キャンセル」）
    pub blocked: Vec<LargeFileItem>,
    /// 警告閾値を超えるが保存できるファイル（E08: 「このまま保存」か「外す」）
    pub warned: Vec<LargeFileItem>,
}

/// 大きいファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct LargeFileItem {
    /// プロジェクトからの相対パス
    pub path: String,
    /// ファイルサイズ（バイト）
    pub size: f64,
}

/// 大きいファイルについての選択。既定（何も選ばない）は、問題のあるファイルがあれば保存しない。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SaveSizeChoice {
    /// 警告（E08）のファイルをそのまま保存する
    pub accept_warned: bool,
    /// 保存対象から外すファイル（検査で見つかったファイルのパスのみ）。.gitignore に追記され、
    /// ファイル自体は消えない
    pub exclude: Vec<String>,
}

/// 取り込み結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct PullResult {
    pub outcome: String,
    pub conflicts: Vec<ConflictItem>,
    /// 取り込み前の自動保存に大きいファイルがあり、何も変更せず見送った場合の内容（E07 / E08）。なければ null
    pub size_check: Option<SizeCheckResult>,
}

/// アップロード結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct PushResult {
    pub outcome: String,
    /// 取り込み前の自動保存に大きいファイルがあり、何も変更せず見送った場合の内容。なければ null
    pub size_check: Option<SizeCheckResult>,
}

/// 履歴アイテム
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct HistoryItem {
    pub commit: String,
    pub timestamp: String,
    pub message: String,
    pub changed_files_count: u32,
    pub is_snapshot: bool,
    /// この保存を作った PC の名前。記録が無ければ null。`message` には含まれない
    pub pc_name: Option<String>,
    /// クラウドに上がっている保存か。自動保存と、クラウドの保管場所が無い・まだ何も上げていないときは false
    pub cloud_synced: bool,
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
    /// この PC 側の最終保存を作った PC の名前。記録が無ければ null
    pub this_pc_name: Option<String>,
    /// クラウド側の最終保存を作った PC の名前。記録が無ければ null
    pub cloud_pc_name: Option<String>,
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
    /// 前回のアプリ終了で途中で止まった操作の名前（`save` / `pull` など）。なければ null（E15）
    pub interrupted_operation: Option<String>,
    /// 登録したフォルダが見つからない・フォルダでない・リポジトリでない（E11）。true のとき git は実行していない
    pub folder_missing: bool,
}

/// 1 ファイルを戻す影響の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreFileKindData {
    /// いまのファイルを指定時点の内容で置き換える
    Overwrite,
    /// いまは無いファイルを作り直す
    Recreate,
    /// すでに同じ内容
    Unchanged,
    /// 指定時点には存在しない（戻せない）
    NotInThatPoint,
}

/// 1 ファイルを戻す操作のプレビュー
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RestoreFilePreviewData {
    pub kind: RestoreFileKindData,
    /// いまのファイルのサイズ（バイト）。無ければ null
    pub size_now: Option<f64>,
    /// 指定時点のファイルのサイズ（バイト）。存在しなければ null
    pub size_then: Option<f64>,
}

/// 1 ファイルを戻した結果の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreFileResultKind {
    /// 戻した
    Restored,
    /// 指定時点に無いため何も変更しなかった
    NotInThatPoint,
    /// 同名の保存対象外ファイルを上書きしてしまうため何も変更しなかった
    IgnoredFileInTheWay,
}

/// 1 ファイルを戻した結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RestoreFileResult {
    pub outcome: RestoreFileResultKind,
    /// 取り消しに使う復元点（`undo_restore` の `restore_point` に渡す）。戻していなければ null
    pub undo_token: Option<String>,
}

/// 全体を元に戻した結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RestoreAllResult {
    /// 取り消しに使う復元点（`undo_restore` の `restore_point` に渡す）
    pub undo_token: Option<String>,
}

/// 保存時点で変更されたファイルの種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum PointChangeKindData {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// 保存時点で変更されたファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PointChangeItem {
    pub path: String,
    /// 名前変更の場合の元のパス
    pub old_path: Option<String>,
    pub kind: PointChangeKindData,
}

/// 追加できたファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AddedFileItem {
    /// プロジェクトからの相対パス
    pub path: String,
    /// 同名があったため別名にした
    pub renamed: bool,
    /// 大きいファイル（50MB 以上）。アップロードに時間がかかることを知らせる
    pub large: bool,
}

/// 追加しなかった理由
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AddRejectKind {
    /// 100MB を超えるため追加できない
    TooLarge,
    /// フォルダなど、ファイルではない
    NotAFile,
    /// 読み取れない、またはコピーに失敗した
    Unreadable,
}

/// 追加しなかったファイル
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RejectedFileItem {
    /// 元のファイル名
    pub name: String,
    pub reason: AddRejectKind,
    /// `too-large` のときの元のサイズ（バイト）
    pub size: Option<f64>,
}

/// ファイル追加の結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AddFilesResult {
    pub added: Vec<AddedFileItem>,
    pub rejected: Vec<RejectedFileItem>,
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
            save_with_size_choice,
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
            open_project_file,
            restore_file_preview,
            restore_file,
            undo_restore,
            abort_merge,
            add_files,
            relocate_project,
            list_point_changes,
            open_file_at,
            recover_interrupted,
            github::get_session,
            github::start_login,
            github::wait_login,
            github::cancel_login,
            github::open_login_page,
            github::logout,
            github::list_owners,
            github::list_remote_projects,
            github::clone_project,
            remote::connect_remote,
            app_settings::get_settings,
            app_settings::update_settings,
            app_settings::complete_onboarding,
        ])
        .events(collect_events![
            events::StatusChanged,
            events::OpProgress,
            events::OpFinished,
            events::SyncStateChanged,
            events::NeedsAttention,
            events::CloneProgress,
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

            // 前回のアプリ終了で途中で止まった操作（ジャーナルが「実行中」のまま）を検出する。
            // 画面が開く前に通知しても届かないため、状態（project_status）にも載せる
            let interrupted = store
                .recover_interrupted(&core_store::now_rfc3339())
                .unwrap_or_else(|e| {
                    eprintln!("操作ジャーナルの確認に失敗しました: {e:?}");
                    Vec::new()
                });

            let state = AppState::new(store);
            app.manage(state);

            let mut notified = std::collections::HashSet::new();
            for entry in interrupted {
                if notified.insert(entry.project_id.clone()) {
                    let _ = events::NeedsAttention {
                        project_id: entry.project_id,
                        reason: events::AttentionReason::InterruptedOperation,
                    }
                    .emit(app.handle());
                }
            }

            // 過去の版を開くために書き出した一時ファイルのうち、古いものを削除する
            std::thread::spawn(|| {
                core_ops::cleanup_old_previews(
                    &preview_root(),
                    core_ops::PREVIEW_MAX_AGE,
                    std::time::SystemTime::now(),
                );
            });

            // 起動時・定期の取り込みとアップロードを開始（実行は操作キュー経由）
            scheduler::spawn(app.handle().clone());

            Ok(())
        })
        .invoke_handler(invoke_handler)
        .run(tauri::generate_context!())
        .expect("アプリの起動に失敗しました");
}
