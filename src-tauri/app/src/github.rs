// GitHub 連携のコマンド: ログイン（Device Flow）、セッション、保存先・リポジトリの一覧、取得（clone）。
//
// トークンの扱い（安全上の不変条件 9）:
// - 保存先は OS のキーチェーン（core-github の TokenStore）だけ。ファイル・ログ・ジャーナル・イベントには出さない
// - 画面へ返す型（SessionInfo など）にトークンも device_code も含めない
// - git へは URL やコマンドライン引数ではなく、credential helper（アプリ自身）経由で渡す
// - エラーは AuthError の種類だけを文言に変換する（通信の本文や URL は載せない）

use std::path::PathBuf;
use std::time::Duration;

use core_git::GitError;
use core_github::{
    clone_url_for, resolve_client_id, AccessToken, AuthError, CreateBlockReason, DeviceFlowClient,
    GithubApi, LoginEnd, OwnerKind, TokenStore, User, UserClient,
};
use core_ops::{CloneDestinationError, Identity, Ops, OpsError};
use core_store::{
    now_rfc3339, redact, JournalOutcome, JournalTrigger, NewJournalEntry, StoreError,
};
use core_watch::FailureKind;
use serde::{Deserialize, Serialize};
use tauri_specta::Event;

use crate::events::{ClonePhase, CloneProgress, StatusChanged};
use crate::{git_runner, run_blocking, run_exclusive, AppError, AppState, ProjectInfo};

/// Hikae の GitHub OAuth App の client_id（公開情報。秘密ではない）。
/// 配布版では利用者が環境変数を設定できないため、既定値として埋め込む。
/// client secret は Device Flow では使わないので、ここにもリポジトリにも置かない。
const DEFAULT_GITHUB_CLIENT_ID: &str = "Ov23lizVk3Q3GQCgYzib";

/// GitHub OAuth App の client_id。実行時の環境変数 `HIKAE_GITHUB_CLIENT_ID` を優先し、
/// 無ければビルド時に同名の環境変数で埋め込まれた値、最後に既定値を使う。
fn client_id() -> Result<String, AuthError> {
    resolve_client_id(Some(
        option_env!("HIKAE_GITHUB_CLIENT_ID").unwrap_or(DEFAULT_GITHUB_CLIENT_ID),
    ))
}

/// clone は大きいリポジトリで時間がかかるため、通常の 60 秒より長く待つ。
const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

// ========== エラーの変換（設計書 5章 E01〜E04） ==========

/// `AuthError` を「何が起きたか／データは無事か／次の行動」の 3 要素へ変換する。
/// 技術情報には種類名だけを入れる（`AuthError` はトークンや応答本文を持たない）。
pub(crate) fn auth_error(e: AuthError, data_is_safe: &str) -> AppError {
    let (what_happened, next_action) = match e {
        // E01
        AuthError::Unauthorized => (
            "GitHub との接続が切れました。",
            "もう一度ログインしてください",
        ),
        // E02
        AuthError::Forbidden => (
            "GitHub がこの操作を許可しませんでした。Organization の管理者による許可が必要な場合があります。",
            "Organization の管理者に確認してください",
        ),
        AuthError::RateLimited => (
            "GitHub への問い合わせが短時間に多すぎました。",
            "しばらく待ってから、もう一度お試しください",
        ),
        // E03
        AuthError::NetworkUnavailable => (
            "インターネットに接続できません。",
            "接続を確認してから、もう一度お試しください",
        ),
        // E04
        AuthError::ServerUnavailable => (
            "GitHub が一時的に応答していません。",
            "少し時間をおいてから、もう一度お試しください",
        ),
        AuthError::MissingClientId | AuthError::DeviceFlowDisabled => (
            "GitHub にログインするための設定が見つかりません。",
            "アプリの提供元に連絡してください",
        ),
        AuthError::KeyringError
        | AuthError::FailedToSaveToken
        | AuthError::FailedToLoadToken
        | AuthError::FailedToDeleteToken => (
            "ログイン情報を PC の安全な保管場所とやり取りできませんでした。",
            "もう一度お試しください。解決しない場合は、PC を再起動してからお試しください",
        ),
        AuthError::NoPendingLogin | AuthError::LoginInProgress => (
            "ログインの手続きが進行中ではありません。",
            "もう一度ログインを始めてください",
        ),
        _ => (
            "GitHub との通信に失敗しました。",
            "しばらくしてから、もう一度お試しください",
        ),
    };
    let code = match &e {
        AuthError::Unauthorized => "not_logged_in",
        AuthError::Forbidden => "github_forbidden",
        AuthError::RateLimited => "github_rate_limited",
        AuthError::NetworkUnavailable => "network_unavailable",
        AuthError::ServerUnavailable => "github_unavailable",
        AuthError::MissingClientId | AuthError::DeviceFlowDisabled => "login_not_configured",
        AuthError::KeyringError
        | AuthError::FailedToSaveToken
        | AuthError::FailedToLoadToken
        | AuthError::FailedToDeleteToken => "keychain_error",
        AuthError::NoPendingLogin | AuthError::LoginInProgress => "login_not_in_progress",
        _ => "github_error",
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

const FILES_SAFE: &str = "ファイルはこの PC に安全に残っています。";

fn store_error(e: StoreError) -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: FILES_SAFE.to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}

fn lock_error() -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: FILES_SAFE.to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: Some("lock".to_string()),
    }
}

// ========== トークン・ユーザー ==========

/// キーチェーンからトークンを読む（呼び出し側は使い終わったらすぐ捨てる）。
async fn load_token() -> Result<Option<AccessToken>, AppError> {
    run_blocking(|| TokenStore::load().map_err(|e| auth_error(e, FILES_SAFE))).await
}

/// ログイン済みであることを求める。未ログインは E01 として扱う。
async fn require_token() -> Result<AccessToken, AppError> {
    load_token()
        .await?
        .ok_or_else(|| auth_error(AuthError::Unauthorized, FILES_SAFE))
}

fn cached_user(state: &AppState) -> Option<User> {
    state.session_user.lock().ok().and_then(|g| g.clone())
}

fn set_cached_user(state: &AppState, user: Option<User>) {
    if let Ok(mut guard) = state.session_user.lock() {
        *guard = user;
    }
}

/// ログイン中のユーザー。控えがあればそれを、無ければ GitHub に問い合わせる。
/// トークンが無効だった場合は控えを捨てる。
async fn current_user(state: &AppState, token: &AccessToken) -> Result<User, AuthError> {
    if let Some(user) = cached_user(state) {
        return Ok(user);
    }
    match UserClient::new().fetch_user(token).await {
        Ok(user) => {
            set_cached_user(state, Some(user.clone()));
            Ok(user)
        }
        Err(e) => {
            if matches!(e, AuthError::Unauthorized) {
                set_cached_user(state, None);
            }
            Err(e)
        }
    }
}

/// 署名に使うユーザーの確認結果。ログイン済みなら (数値 ID, ログイン名)、確認できなければ None。
/// 未ログイン・オフライン・トークン失効はすべて None にし、エラーにはしない（トークンは扱わない）。
pub(crate) async fn signing_user(state: &AppState) -> Option<(u64, String)> {
    if let Some(user) = cached_user(state) {
        return Some((user.id, user.login));
    }
    let token = load_token().await.ok().flatten()?;
    let user = current_user(state, &token).await.ok()?;
    Some((user.id, user.login))
}

/// 控えているユーザーだけから署名用の情報を返す（通信しない）。保存のたびに呼ぶため。
pub(crate) fn cached_signing_user(state: &AppState) -> Option<(u64, String)> {
    cached_user(state).map(|u| (u.id, u.login))
}

// ========== セッション・ログイン ==========

/// ログイン中の GitHub ユーザー（表示用）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SessionUser {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// 現在のログイン状態。トークンは含めない。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SessionInfo {
    /// ログイン済みか（オフラインでユーザー情報を確認できない場合も true）
    pub logged_in: bool,
    /// トークンが失効・取り消しされており、もう一度ログインが必要（E01）
    pub reauth_required: bool,
    /// 初回設定を完了したか
    pub onboarded: bool,
    /// ユーザー情報。未ログイン、またはオフラインで確認できないときは null
    pub user: Option<SessionUser>,
}

fn session_user(user: &User) -> SessionUser {
    SessionUser {
        login: user.login.clone(),
        avatar_url: user.avatar_url.clone(),
    }
}

/// ログイン状態を返す。
#[tauri::command]
#[specta::specta]
pub async fn get_session(state: tauri::State<'_, AppState>) -> Result<SessionInfo, AppError> {
    let store = state.store_clone();
    let onboarded = run_blocking(move || {
        let guard = store.lock().map_err(|_| lock_error())?;
        guard
            .get_settings()
            .map(|s| s.onboarded)
            .map_err(store_error)
    })
    .await?;

    let Some(token) = load_token().await? else {
        set_cached_user(&state, None);
        return Ok(SessionInfo {
            logged_in: false,
            reauth_required: false,
            onboarded,
            user: None,
        });
    };

    if let Some(user) = cached_user(&state) {
        return Ok(SessionInfo {
            logged_in: true,
            reauth_required: false,
            onboarded,
            user: Some(session_user(&user)),
        });
    }

    match UserClient::new().fetch_user(&token).await {
        Ok(user) => {
            set_cached_user(&state, Some(user.clone()));
            Ok(SessionInfo {
                logged_in: true,
                reauth_required: false,
                onboarded,
                user: Some(session_user(&user)),
            })
        }
        // トークンが無効: 取り消された。保存されたトークンは残し、再ログインで上書きする
        Err(AuthError::Unauthorized) => {
            set_cached_user(&state, None);
            Ok(SessionInfo {
                logged_in: false,
                reauth_required: true,
                onboarded,
                user: None,
            })
        }
        // オフラインや GitHub 側の障害では、ログイン状態を変えない
        Err(_) => Ok(SessionInfo {
            logged_in: true,
            reauth_required: false,
            onboarded,
            user: None,
        }),
    }
}

/// 画面に出すログイン手順（device_code は含まない）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct LoginStart {
    /// GitHub の画面で入力するコード
    pub user_code: String,
    /// コードを入力するページの URL
    pub verification_uri: String,
    /// 有効期限（秒）
    pub expires_in: u32,
    /// ポーリング間隔（秒）
    pub interval: u32,
}

/// ログインの結末
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum LoginOutcome {
    /// ログインできた
    Succeeded,
    /// GitHub の画面で拒否された
    Denied,
    /// 有効期限が切れた
    Expired,
    /// キャンセルした
    Canceled,
}

fn clamp_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// ログインを始める（Device Flow）。確認コードと確認ページの URL を返す。
#[tauri::command]
#[specta::specta]
pub async fn start_login(state: tauri::State<'_, AppState>) -> Result<LoginStart, AppError> {
    let client = DeviceFlowClient::new(client_id().map_err(|e| auth_error(e, FILES_SAFE))?);
    let prompt = state
        .login
        .start(&client)
        .await
        .map_err(|e| auth_error(e, FILES_SAFE))?;
    Ok(LoginStart {
        user_code: prompt.user_code,
        verification_uri: prompt.verification_uri,
        expires_in: clamp_u32(prompt.expires_in),
        interval: clamp_u32(prompt.interval),
    })
}

/// ユーザーが GitHub で許可するまで待つ。長時間かかるため async のまま待機し、`cancel_login` で中断できる。
#[tauri::command]
#[specta::specta]
pub async fn wait_login(state: tauri::State<'_, AppState>) -> Result<LoginOutcome, AppError> {
    let client = DeviceFlowClient::new(client_id().map_err(|e| auth_error(e, FILES_SAFE))?);
    let login = state.login.clone();
    let end = login
        .wait(&client)
        .await
        .map_err(|e| auth_error(e, FILES_SAFE))?;

    match end {
        LoginEnd::Token(token) => {
            // トークンはキーチェーンにだけ保存する
            let to_save = token.clone();
            run_blocking(move || TokenStore::save(&to_save).map_err(|e| auth_error(e, FILES_SAFE)))
                .await?;
            // ユーザー情報の取得に失敗してもログイン自体は成功（次回の get_session で再取得する）
            match UserClient::new().fetch_user(&token).await {
                Ok(user) => set_cached_user(&state, Some(user)),
                Err(_) => set_cached_user(&state, None),
            }
            Ok(LoginOutcome::Succeeded)
        }
        LoginEnd::Denied => Ok(LoginOutcome::Denied),
        LoginEnd::Expired => Ok(LoginOutcome::Expired),
        LoginEnd::Canceled => Ok(LoginOutcome::Canceled),
    }
}

/// ログインの確認ページ（https://github.com/login/device）を既定のブラウザで開く。
/// 任意の URL は受け取らない。開くのは進行中のログインが保持している確認 URL だけで、
/// ホストとパスが固定の確認ページと完全一致する場合に限る。
#[tauri::command]
#[specta::specta]
pub fn open_login_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;

    let fail = |code: &str, what: &str, next: &str, technical: Option<String>| AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: what.to_string(),
        data_is_safe: FILES_SAFE.to_string(),
        next_action: next.to_string(),
        technical_info: technical,
    };
    let Some(url) = state.login.verification_uri() else {
        return Err(fail(
            "login_not_in_progress",
            "ログインの手続きが進行中ではありません。",
            "もう一度ログインを始めてください",
            None,
        ));
    };
    if !core_github::is_login_page_url(&url) {
        return Err(fail(
            "login_page_unexpected",
            "ログインのページを開けませんでした。",
            "表示されているページを、ブラウザで手動で開いてください",
            Some("unexpected verification url".to_string()),
        ));
    }
    app.opener()
        .open_url(core_github::LOGIN_PAGE_URL, None::<&str>)
        .map_err(|e| {
            fail(
                "browser_open_failed",
                "ブラウザを開けませんでした。",
                "表示されているページを、ブラウザで手動で開いてください",
                Some(e.to_string()),
            )
        })
}

/// 進行中のログインを中断する。待機中の `wait_login` は `canceled` で戻る。
#[tauri::command]
#[specta::specta]
pub fn cancel_login(state: tauri::State<'_, AppState>) {
    state.login.cancel();
}

/// ログアウトする。キーチェーンのトークンを削除する（GitHub 側の許可の取り消しは行わない）。
#[tauri::command]
#[specta::specta]
pub async fn logout(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.login.cancel();
    run_blocking(|| TokenStore::delete().map_err(|e| auth_error(e, FILES_SAFE))).await?;
    set_cached_user(&state, None);
    Ok(())
}

// ========== 保存先・リポジトリの一覧 ==========

/// 保存先の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerKindData {
    Personal,
    Org,
}

/// 保存先に作れない理由
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerBlockReason {
    /// Organization への参加が承認待ち
    PendingInvitation,
    /// Organization の設定でメンバーによる作成が許可されていない
    MembersCannotCreate,
    /// Organization の情報を確認できない（メンバーでない、またはアプリの利用が許可されていない）
    NoAccess,
}

/// 保存先（個人または Organization）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OwnerInfo {
    /// GitHub のログイン名。`RemoteProjectInfo.owner_id` と一致する
    pub id: String,
    pub name: String,
    pub kind: OwnerKindData,
    pub avatar_url: Option<String>,
    /// 新しく作れるか。false のときは灰色表示にして `reason` を説明する
    pub can_create: bool,
    pub reason: Option<OwnerBlockReason>,
}

/// 保存先（個人 + 所属 Organization）の一覧。
#[tauri::command]
#[specta::specta]
pub async fn list_owners(state: tauri::State<'_, AppState>) -> Result<Vec<OwnerInfo>, AppError> {
    let token = require_token().await?;
    let owners = GithubApi::new()
        .list_owners(&token)
        .await
        .map_err(|e| github_read_error(&state, e))?;
    Ok(owners
        .into_iter()
        .map(|o| OwnerInfo {
            name: o.login.clone(),
            id: o.login,
            kind: match o.kind {
                OwnerKind::Personal => OwnerKindData::Personal,
                OwnerKind::Org => OwnerKindData::Org,
            },
            avatar_url: o.avatar_url,
            can_create: o.can_create,
            reason: o.block_reason.map(|r| match r {
                CreateBlockReason::PendingInvitation => OwnerBlockReason::PendingInvitation,
                CreateBlockReason::MembersCannotCreate => OwnerBlockReason::MembersCannotCreate,
                CreateBlockReason::NoAccess => OwnerBlockReason::NoAccess,
            }),
        })
        .collect())
}

/// 読み取り系の GitHub 呼び出しの失敗を変換する。トークンが無効なら控えのユーザー情報も捨てる。
fn github_read_error(state: &AppState, e: AuthError) -> AppError {
    if matches!(e, AuthError::Unauthorized) {
        set_cached_user(state, None);
    }
    auth_error(e, FILES_SAFE)
}

/// 取得できるリポジトリ
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoteProjectInfo {
    /// `owner/name`。`clone_project` の `repo` に渡す
    pub id: String,
    pub name: String,
    /// 所有者のログイン名（`OwnerInfo.id`）
    pub owner_id: String,
    pub private: bool,
    /// 最終更新日時（RFC3339）
    pub updated_at: Option<String>,
}

/// 取得できるリポジトリの一覧
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoteProjectList {
    /// オーナーごとにまとまった並び（個人が先、Organization は名前順。各オーナー内も名前順）
    pub projects: Vec<RemoteProjectInfo>,
    /// 件数が多く、一部しか取得していない
    pub truncated: bool,
}

/// 取得できるリポジトリの一覧（`query` を含む `owner/name` に絞り込む）。
#[tauri::command]
#[specta::specta]
pub async fn list_remote_projects(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<RemoteProjectList, AppError> {
    let token = require_token().await?;
    let list = GithubApi::new()
        .list_repos(&token, &query)
        .await
        .map_err(|e| github_read_error(&state, e))?;
    Ok(RemoteProjectList {
        truncated: list.truncated,
        projects: list
            .repos
            .into_iter()
            .map(|r| RemoteProjectInfo {
                id: r.full_name,
                name: r.name,
                owner_id: r.owner_login,
                private: r.private,
                updated_at: r.updated_at,
            })
            .collect(),
    })
}

// ========== GitHub から取得（clone） ==========

fn emit_clone(app: &tauri::AppHandle, project_id: &str, phase: ClonePhase) {
    let _ = CloneProgress {
        project_id: project_id.to_string(),
        phase,
    }
    .emit(app);
}

fn clone_input_error(code: &str, what: &str, next: &str) -> AppError {
    AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: what.to_string(),
        data_is_safe: "ファイルは変更されていません。".to_string(),
        next_action: next.to_string(),
        technical_info: None,
    }
}

/// 取得の失敗を AppError へ変換する。認証・通信の失敗は E01 / E03、保管場所が無い場合は E16 の文言にする。
fn clone_failure(e: OpsError) -> AppError {
    let safe = "この PC のファイルは変更されていません。";
    if let OpsError::Git(GitError::Failed { stderr, .. }) = &e {
        let technical = Some(redact(stderr));
        let lower = stderr.to_ascii_lowercase();
        let (code, what, next) = match core_watch::classify_failure(stderr) {
            FailureKind::AuthFailed => (
                "not_logged_in",
                "GitHub との接続が切れました。",
                "もう一度ログインしてください",
            ),
            FailureKind::Offline => (
                "network_unavailable",
                "インターネットに接続できません。",
                "接続を確認してから、もう一度お試しください",
            ),
            _ if lower.contains("not found") => (
                "remote_not_found",
                "クラウドの保管場所が見つかりません。GitHub 上で削除または名前変更された可能性があります。",
                "一覧から選び直してください",
            ),
            _ => (
                "clone_failed",
                "GitHub からの取得に失敗しました。",
                "しばらくしてから、もう一度お試しください",
            ),
        };
        return AppError {
            code: code.to_string(),
            params: Vec::new(),
            what_happened: what.to_string(),
            data_is_safe: safe.to_string(),
            next_action: next.to_string(),
            technical_info: technical,
        };
    }
    if matches!(e, OpsError::Git(GitError::Timeout { .. })) {
        return AppError {
            code: "clone_timeout".to_string(),
            params: Vec::new(),
            what_happened: "取得に時間がかかりすぎたため中断しました。".to_string(),
            data_is_safe: safe.to_string(),
            next_action: "接続を確認してから、もう一度お試しください".to_string(),
            technical_info: Some("timeout".to_string()),
        };
    }
    AppError::from_ops_error(e)
}

/// GitHub のリポジトリをこの PC のフォルダへ取得し、プロジェクトとして登録する。
///
/// - `repo` は `owner/name`。URL は常に `https://github.com/...` に固定し、任意の URL は受け付けない
/// - `path` は取得先のフォルダ。存在しないか空である必要がある（空でなければ拒否し、中身には触れない）
/// - 認証は credential helper 経由（トークンを URL や引数に載せない）
/// - 新しいフォルダを作るだけで既存のファイルを変更しないため、復元点は作らない
#[tauri::command]
#[specta::specta]
pub async fn clone_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    repo: String,
    path: PathBuf,
    display_name: Option<String>,
) -> Result<ProjectInfo, AppError> {
    let Some(url) = clone_url_for(&repo) else {
        return Err(clone_input_error(
            "clone_invalid_repo",
            "取得するプロジェクトの指定が正しくありません。",
            "一覧から選び直してください",
        ));
    };
    if !path.is_absolute() {
        return Err(clone_input_error(
            "clone_invalid_destination",
            "取得先のフォルダの指定が正しくありません。",
            "フォルダを選び直してください",
        ));
    }
    let repo_name = repo.split('/').nth(1).unwrap_or_default().to_string();
    let display_name = display_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or(repo_name);

    emit_clone(&app, &id, ClonePhase::Preparing);

    // ログイン済みであること、署名に使うユーザー名とメールアドレス（noreply）を確認する
    let user = match require_token().await {
        Ok(token) => current_user(&state, &token)
            .await
            .map_err(|e| auth_error(e, FILES_SAFE)),
        Err(e) => Err(e),
    };
    let user = match user {
        Ok(user) => user,
        Err(e) => {
            emit_clone(&app, &id, ClonePhase::Failed);
            return Err(e);
        }
    };
    let identity = Identity {
        name: user.login.clone(),
        email: user.noreply_email(),
    };

    let store = state.store_clone();
    let locks = state.locks_clone();
    let task_app = app.clone();
    let task_id = id.clone();
    let owner = user.login.clone();
    let outcome = run_exclusive(
        locks,
        id.clone(),
        "この PC のファイルは変更されていません。",
        move || {
            let started_at = now_rfc3339();

            // 既に登録済みの ID・フォルダは拒否する（取得したあとで登録に失敗しないように先に確認）
            {
                let guard = store.lock().map_err(|_| lock_error())?;
                if guard.get_project(&task_id).is_ok() {
                    return Err(clone_input_error(
                        "project_already_registered",
                        "このプロジェクトはすでに登録されています。",
                        "一覧から開いてください",
                    ));
                }
                let registered = guard.list_projects().map_err(store_error)?;
                if registered.iter().any(|p| p.path == path) {
                    return Err(clone_input_error(
                        "folder_already_registered",
                        "このフォルダはすでに別のプロジェクトとして登録されています。",
                        "別のフォルダを選んでください",
                    ));
                }
            }
            // 取得先は存在しないか空であること
            if let Err(reason) = core_ops::check_clone_destination(&path) {
                let (code, what) = match reason {
                    CloneDestinationError::NotEmpty => (
                        "destination_not_empty",
                        "選んだフォルダには、すでにファイルが入っています。",
                    ),
                    CloneDestinationError::NotADirectory => (
                        "destination_not_a_folder",
                        "選んだ場所はフォルダではありません。",
                    ),
                    CloneDestinationError::Unreadable => (
                        "destination_unreadable",
                        "選んだフォルダの中身を確認できません。",
                    ),
                };
                return Err(clone_input_error(
                    code,
                    what,
                    "空のフォルダを選び直してください",
                ));
            }

            emit_clone(&task_app, &task_id, ClonePhase::Downloading);
            let runner = git_runner().with_timeout(CLONE_TIMEOUT);
            let ops = Ops::new(runner);
            ops.clone_project(&url, &path, &identity)
                .map_err(clone_failure)?;

            emit_clone(&task_app, &task_id, ClonePhase::Finishing);
            let branch = git_runner()
                .run(&path, &["symbolic-ref", "--short", "HEAD"])
                .ok()
                .filter(|o| o.code == 0)
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".to_string());
            let initial_commit = ops.initial_commit(&path).ok().flatten();

            let guard = store.lock().map_err(|_| lock_error())?;
            guard
                .add_project(&task_id, &display_name, &path, Some(&url), &owner, &branch)
                .map_err(|e| AppError {
                    code: "clone_register_failed".to_string(),
                    params: Vec::new(),
                    what_happened: "取得はできましたが、プロジェクトの登録に失敗しました。"
                        .to_string(),
                    data_is_safe: "取得したファイルはフォルダに残っています。".to_string(),
                    next_action: "「既存のフォルダを登録」から、そのフォルダを登録してください"
                        .to_string(),
                    technical_info: Some(format!("{e:?}")),
                })?;
            if let Some(oid) = initial_commit {
                let _ = guard.set_initial_commit(&task_id, &oid);
            }
            // 操作ジャーナルへ記録する。失敗しても取得の結果は変えない
            let _ = guard.record_journal(&NewJournalEntry {
                project_id: task_id.clone(),
                operation: "clone".to_string(),
                trigger: JournalTrigger::Manual,
                started_at,
                finished_at: now_rfc3339(),
                outcome: JournalOutcome::Success,
                detail: Some("cloned".to_string()),
                snapshot_ref: None,
                backup_ref: None,
                target: Some(repo.clone()),
            });
            let project = guard.get_project(&task_id).map_err(store_error)?;
            Ok(ProjectInfo {
                id: project.id,
                display_name: project.display_name,
                path: project.path,
                remote_url: project.remote_url,
                owner: project.owner,
                last_viewed_at: project.last_viewed_at,
            })
        },
    )
    .await;

    match outcome {
        Ok(info) => {
            emit_clone(&app, &id, ClonePhase::Done);
            let _ = StatusChanged {
                project_id: info.id.clone(),
            }
            .emit(&app);
            // 取り込み・アップロードの計画に加える
            state.scheduler.refresh();
            Ok(info)
        }
        Err(e) => {
            emit_clone(&app, &id, ClonePhase::Failed);
            Err(e)
        }
    }
}
