// モバイル版の Tauri コマンド（設計書 14.8）。トークンは Rust 側だけで持ち、画面には渡さない。
// GitHub への通信はすべて core-remote / core-github を通る（git は使わない）。

use std::sync::{Arc, Mutex};

use base64::Engine;
use core_github::{
    is_login_page_url, resolve_client_id, AccessToken, AuthError, DeviceFlowClient,
    LoginCoordinator, LoginEnd, User, UserClient, LOGIN_PAGE_URL,
};
use core_remote::{preview, PlanItem, RemoteClient, UploadItem, UploadRequest};

use crate::dto::{
    DirListing, LoginOutcome, LoginStart, MobileAccount, MobileDownload, MobilePreview, MobileRepo,
    MobileVersion, PlanItemInput, UploadItemInput, UploadPlan, UploadResult,
};
use crate::error::MobileError;
use crate::tokens::Tokens;

/// Hikae の GitHub OAuth App の client_id（公開情報。秘密ではない。デスクトップ版と同じもの）。
/// Device Flow では client secret を使わないので、ここにもリポジトリにも置かない
const DEFAULT_GITHUB_CLIENT_ID: &str = "Ov23lizVk3Q3GQCgYzib";

fn client_id() -> Result<String, AuthError> {
    resolve_client_id(Some(
        option_env!("HIKAE_GITHUB_CLIENT_ID").unwrap_or(DEFAULT_GITHUB_CLIENT_ID),
    ))
}

/// アプリの状態。トークンは持たない（毎回、保管先から読んで使い終わったら捨てる）
pub struct MobileState {
    tokens: Tokens,
    login: Arc<LoginCoordinator>,
    user: Mutex<Option<User>>,
    remote: RemoteClient,
}

impl MobileState {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            tokens: Tokens::new(app),
            login: Arc::new(LoginCoordinator::new()),
            user: Mutex::new(None),
            remote: RemoteClient::new(),
        }
    }

    fn cached_user(&self) -> Option<User> {
        self.user.lock().ok().and_then(|g| g.clone())
    }

    fn set_user(&self, user: Option<User>) {
        if let Ok(mut guard) = self.user.lock() {
            *guard = user;
        }
    }
}

async fn require_token(state: &MobileState) -> Result<AccessToken, MobileError> {
    state
        .tokens
        .load()
        .await?
        .ok_or_else(|| MobileError::from(AuthError::Unauthorized))
}

/// ログイン中のユーザー。控えがあればそれを、無ければ GitHub に問い合わせる
async fn current_user(state: &MobileState, token: &AccessToken) -> Result<User, MobileError> {
    if let Some(user) = state.cached_user() {
        return Ok(user);
    }
    match UserClient::new().fetch_user(token).await {
        Ok(user) => {
            state.set_user(Some(user.clone()));
            Ok(user)
        }
        Err(e) => {
            if matches!(e, AuthError::Unauthorized) {
                state.set_user(None);
            }
            Err(e.into())
        }
    }
}

fn clamp_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

// ========== ログイン ==========

/// 現在のアカウント。未ログイン、またはトークンが失効しているときは null
#[tauri::command]
#[specta::specta]
pub async fn mobile_account(
    state: tauri::State<'_, MobileState>,
) -> Result<Option<MobileAccount>, MobileError> {
    let Some(token) = state.tokens.load().await? else {
        return Ok(None);
    };
    match current_user(&state, &token).await {
        Ok(user) => Ok(Some(MobileAccount::from(&user))),
        Err(e) if e.code == "not_logged_in" => Ok(None),
        Err(e) => Err(e),
    }
}

/// ログインを始める（Device Flow）。確認コードと確認ページの URL を返す
#[tauri::command]
#[specta::specta]
pub async fn mobile_start_login(
    state: tauri::State<'_, MobileState>,
) -> Result<LoginStart, MobileError> {
    let client = DeviceFlowClient::new(client_id()?);
    let prompt = state.login.start(&client).await?;
    Ok(LoginStart {
        user_code: prompt.user_code,
        verification_uri: prompt.verification_uri,
        expires_in: clamp_u32(prompt.expires_in),
        interval: clamp_u32(prompt.interval),
    })
}

/// 確認ページ（https://github.com/login/device）をブラウザで開く。
/// 任意の URL は受け取らず、進行中のログインが保持する URL が固定の確認ページと一致するときだけ開く
#[tauri::command]
#[specta::specta]
pub fn mobile_open_login_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, MobileState>,
) -> Result<(), MobileError> {
    use tauri_plugin_opener::OpenerExt;

    let Some(url) = state.login.verification_uri() else {
        return Err(MobileError::login_not_in_progress());
    };
    if !is_login_page_url(&url) {
        return Err(MobileError::login_page_unexpected());
    }
    app.opener()
        .open_url(LOGIN_PAGE_URL, None::<&str>)
        .map_err(|e| MobileError::browser_open_failed(e.to_string()))
}

/// 許可されるまで待つ。`mobile_cancel_login` で中断できる
#[tauri::command]
#[specta::specta]
pub async fn mobile_wait_login(
    state: tauri::State<'_, MobileState>,
) -> Result<LoginOutcome, MobileError> {
    let client = DeviceFlowClient::new(client_id()?);
    let login = state.login.clone();
    match login.wait(&client).await? {
        LoginEnd::Token(token) => {
            // トークンは端末の安全な保管先にだけ保存する
            let to_save = token.clone();
            state.tokens.save(to_save).await?;
            // ユーザー情報の取得に失敗してもログイン自体は成功（次回の mobile_account で再取得する）
            match UserClient::new().fetch_user(&token).await {
                Ok(user) => state.set_user(Some(user)),
                Err(_) => state.set_user(None),
            }
            Ok(LoginOutcome::Succeeded)
        }
        LoginEnd::Denied => Ok(LoginOutcome::Denied),
        LoginEnd::Expired => Ok(LoginOutcome::Expired),
        LoginEnd::Canceled => Ok(LoginOutcome::Canceled),
    }
}

#[tauri::command]
#[specta::specta]
pub fn mobile_cancel_login(state: tauri::State<'_, MobileState>) {
    state.login.cancel();
}

/// ログアウト。保管先のトークンを削除する（GitHub 側の許可の取り消しは行わない）
#[tauri::command]
#[specta::specta]
pub async fn mobile_logout(state: tauri::State<'_, MobileState>) -> Result<(), MobileError> {
    state.login.cancel();
    state.tokens.delete().await?;
    state.set_user(None);
    Ok(())
}

// ========== 閲覧 ==========

#[tauri::command]
#[specta::specta]
pub async fn mobile_list_repos(
    state: tauri::State<'_, MobileState>,
) -> Result<Vec<MobileRepo>, MobileError> {
    let token = require_token(&state).await?;
    let repos = state.remote.list_repos(&token).await?;
    Ok(repos.into_iter().map(MobileRepo::from).collect())
}

/// フォルダの一覧。`git_ref` が無いときは既定ブランチの先頭
#[tauri::command]
#[specta::specta]
pub async fn mobile_list_dir(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    path: String,
    git_ref: Option<String>,
) -> Result<DirListing, MobileError> {
    let token = require_token(&state).await?;
    let listing = state
        .remote
        .list_dir(&token, &owner, &name, &path, git_ref.as_deref())
        .await?;
    Ok(listing.into())
}

/// 表示・ダウンロードの対象の内容を読む。`git_ref`（過去の版のコミット）があるときは、
/// その時点のファイルの blob を引いてから読む。無いときは一覧の `sha`（blob の ID）をそのまま使う
async fn read_file_bytes(
    state: &MobileState,
    token: &AccessToken,
    owner: &str,
    name: &str,
    path: &str,
    sha: &str,
    git_ref: Option<&str>,
) -> Result<Vec<u8>, core_remote::RemoteError> {
    match git_ref {
        Some(commit) => {
            let (blob, size) = state
                .remote
                .file_at(token, owner, name, path, commit)
                .await?;
            state
                .remote
                .fetch_blob(token, owner, name, &blob, Some(size))
                .await
        }
        None => state.remote.fetch_blob(token, owner, name, sha, None).await,
    }
}

fn file_name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// ファイルの表示用の内容。形式の判定と抽出は core-remote の `preview` が行う
#[tauri::command]
#[specta::specta]
pub async fn mobile_preview(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    path: String,
    sha: String,
    git_ref: Option<String>,
) -> Result<MobilePreview, MobileError> {
    let token = require_token(&state).await?;
    let bytes = match read_file_bytes(
        &state,
        &token,
        &owner,
        &name,
        &path,
        &sha,
        git_ref.as_deref(),
    )
    .await
    {
        Ok(b) => b,
        Err(core_remote::RemoteError::TooLarge) => return Ok(MobilePreview::TooLarge),
        Err(e) => return Err(e.into()),
    };
    let file_name = file_name_of(&path).to_string();
    // 抽出（zip の展開と XML の解析）は CPU を使うので、ブロッキング用のスレッドで行う
    let built =
        tauri::async_runtime::spawn_blocking(move || preview::build_preview(&file_name, bytes))
            .await
            .map_err(|e| MobileError::task_failed(&format!("{e:?}")))?;
    Ok(crate::preview_dto::to_dto(built))
}

#[tauri::command]
#[specta::specta]
pub async fn mobile_download(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    path: String,
    sha: String,
    git_ref: Option<String>,
) -> Result<MobileDownload, MobileError> {
    let token = require_token(&state).await?;
    let bytes = read_file_bytes(
        &state,
        &token,
        &owner,
        &name,
        &path,
        &sha,
        git_ref.as_deref(),
    )
    .await?;
    let file_name = file_name_of(&path).to_string();
    Ok(MobileDownload {
        mime: preview::mime_for(&file_name).to_string(),
        file_name,
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn mobile_file_history(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    path: String,
) -> Result<Vec<MobileVersion>, MobileError> {
    let token = require_token(&state).await?;
    let versions = state
        .remote
        .file_history(&token, &owner, &name, &path)
        .await?;
    Ok(versions.into_iter().map(MobileVersion::from).collect())
}

// ========== アップロード（追加して保存） ==========

#[tauri::command]
#[specta::specta]
pub async fn mobile_plan_upload(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    items: Vec<PlanItemInput>,
) -> Result<UploadPlan, MobileError> {
    let token = require_token(&state).await?;
    let items: Vec<PlanItem> = items
        .into_iter()
        .map(|i| PlanItem {
            target_dir: i.target_dir,
            name: i.name,
            size: u64::from(i.size),
        })
        .collect();
    let plan = state
        .remote
        .plan_upload(&token, &owner, &name, &items)
        .await?;
    Ok(plan.into())
}

#[tauri::command]
#[specta::specta]
pub async fn mobile_upload(
    state: tauri::State<'_, MobileState>,
    owner: String,
    name: String,
    message: Option<String>,
    items: Vec<UploadItemInput>,
) -> Result<UploadResult, MobileError> {
    let token = require_token(&state).await?;
    let user = current_user(&state, &token).await?;
    let mut upload_items = Vec::with_capacity(items.len());
    for item in items {
        let content = base64::engine::general_purpose::STANDARD
            .decode(item.content_base64.as_bytes())
            .map_err(|_| MobileError::invalid_input("base64"))?;
        upload_items.push(UploadItem {
            target_dir: item.target_dir,
            name: item.name,
            content,
            on_exists: item.on_exists.into(),
        });
    }
    let result = state
        .remote
        .upload(
            &token,
            &user,
            &owner,
            &name,
            UploadRequest {
                message,
                items: upload_items,
            },
        )
        .await?;
    Ok(result.into())
}
