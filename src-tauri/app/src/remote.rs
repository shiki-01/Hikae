// 保存先（GitHub 上のリポジトリ）の作成と接続（設計書 4.6）。
//
// 流れ: (1) GitHub にリポジトリを作る → (2) `origin` を設定する（`run_op`、操作名 `connect-remote`）
//       → (3) 初回の保存（頼まれたときだけ）→ (4) 初回のアップロード（`run_op`、操作名 `push`）。
//
// 安全上の不変条件との対応:
// - 2・3: git は `GitRunner` 経由のみ。`origin` の設定は `.git/config` だけを変え、初回の保存は通常の
//   保存と同じ復元点を作る。アップロードは `push -u origin HEAD`（force 系は許可リストが拒否する）
// - 7: (2) と (4) は `run_op`（= `run_exclusive`）の中で実行する。GitHub の API 呼び出し (1) は
//   作業フォルダにもインデックスにも触れないため、キューの外で行う
// - 9: `origin` の URL は `https://github.com/<owner>/<name>.git` で認証情報を含めない（認証は credential
//   helper）。トークンは API 呼び出しにだけ使い、エラー・ジャーナル・params・イベントには出さない
// - 公開（private=false）は、画面の警告に同意した明示のフラグ（`public_confirmed`）があるときだけ許可する

use core_github::{clone_url_for, suggest_repository_name, AuthError, GithubApi, OwnerKind};
use core_ops::{FirstSave, OpsError, UploadOutcome};
use core_store::redact;
use core_watch::{FailureKind, SyncTask};
use serde::{Deserialize, Serialize};

use crate::github::{auth_error, current_user, require_token, FILES_SAFE};
use crate::ops_runner::{
    failure_kind, run_op, size_limits_for, summarize_push, OpContext, OpFailure, OpSpec, OpSummary,
};
use crate::scheduler::push_task_result;
use crate::{
    load_project, record_initial_commit, run_blocking, size_check_result, AppError, AppState,
    SizeCheckResult,
};

/// 初回の保存のメモ（新しいプロジェクトにすでにファイルがあるときだけ使う）
const FIRST_SAVE_MEMO: &str = "最初の保存";

/// 保存先（GitHub 上のリポジトリ）の作成の要求
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoteRequest {
    /// 保存先（個人のログイン名、または Organization 名）。`list_owners` の `id`
    pub owner: String,
    /// リポジトリ名。省略時はプロジェクト名から作る（英数字・`-`・`_`・`.` のみ。日本語の名前などは
    /// `hikae-<識別子>` にする）
    pub name: Option<String>,
    /// 非公開にする（既定の選択肢）
    pub private: bool,
    /// 公開にすることの警告を確認した。`private` が false のとき、これが true でなければ拒否する
    pub public_confirmed: bool,
}

/// 保存先の接続の結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoteConnectResult {
    /// GitHub 上に作ったリポジトリ（`owner/name`）。作れなかったときは null
    pub repository: Option<String>,
    /// 保存先を設定できた（以降は取り込み・アップロードの対象になる）
    pub connected: bool,
    /// 初回のアップロードまで完了した（アップロードするものが無かったときは false）
    pub uploaded: bool,
    /// 初回の保存に大きいファイルがあり、保存を見送った場合の内容（E07 / E08）。なければ null
    pub size_check: Option<SizeCheckResult>,
    /// 途中で失敗した場合の 3 要素のエラー（E02 / E03 など）。ローカルのファイルは無事
    pub error: Option<AppError>,
}

impl RemoteConnectResult {
    /// 保存先を作れなかった（ローカルの登録だけが残る）
    pub(crate) fn not_connected(error: AppError) -> Self {
        RemoteConnectResult {
            repository: None,
            connected: false,
            uploaded: false,
            size_check: None,
            error: Some(error),
        }
    }
}

/// プロジェクト追加の結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AddProjectResult {
    /// 保存先の作成を頼まれたときだけ入る
    pub remote: Option<RemoteConnectResult>,
}

fn error(code: &str, what: &str, safe: &str, next: &str, technical: Option<String>) -> AppError {
    AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: what.to_string(),
        data_is_safe: safe.to_string(),
        next_action: next.to_string(),
        technical_info: technical,
    }
}

const NOTHING_CREATED: &str =
    "ファイルはこの PC に安全に残っています。GitHub には何も作られていません。";

/// 公開にする要求は、警告を確認した明示のフラグが無ければ拒否する（何も作らない）。
pub(crate) fn check_public_confirmed(request: &RemoteRequest) -> Result<(), AppError> {
    if request.private || request.public_confirmed {
        return Ok(());
    }
    Err(error(
        "remote_public_not_confirmed",
        "公開してよいかの確認が済んでいません。",
        "ファイルは変更されていません。GitHub には何も作られていません。",
        "公開する場合は、注意事項を確認してからもう一度お試しください",
        None,
    ))
}

/// リポジトリ作成の失敗を 3 要素のエラーにする。トークンが無効なら控えのユーザー情報も捨てる。
fn create_error(state: &AppState, e: AuthError) -> AppError {
    if matches!(e, AuthError::Unauthorized) {
        if let Ok(mut guard) = state.session_user.lock() {
            *guard = None;
        }
    }
    auth_error(e, NOTHING_CREATED)
}

/// アップロードの失敗を 3 要素のエラーにする（通信できない: E03、認証: E01）。
/// 保存先は作成済みで、ファイルはこの PC に残っていることを伝える。
fn upload_error(failure: OpFailure) -> AppError {
    let safe = "ファイルはこの PC に安全に残っています。GitHub 上の保存先は作成済みです。";
    let e = match failure {
        OpFailure::App(e) => return e,
        OpFailure::Ops(e) => e,
    };
    let technical = match &e {
        OpsError::Git(core_git::GitError::Failed { stderr, .. }) => Some(redact(stderr)),
        other => Some(redact(&format!("{other:?}"))),
    };
    match failure_kind(&e) {
        FailureKind::Offline => error(
            "network_unavailable",
            "インターネットに接続できません。",
            safe,
            "接続が戻ったら、アップロードしてください",
            technical,
        ),
        FailureKind::AuthFailed => error(
            "not_logged_in",
            "GitHub との接続が切れました。",
            safe,
            "もう一度ログインしてください",
            technical,
        ),
        _ => AppError::from_ops_error(e),
    }
}

/// GitHub 上にリポジトリを作り、プロジェクトの保存先として接続し、初回のアップロードまで行う。
///
/// - リポジトリを作る前に失敗した場合は `Err`（何も作られていない）
/// - 作った後のアップロードの失敗は `Ok` の `error` で返す（保存先は接続済みで、あとからアップロードできる）
/// - `first_save` が true のときは、変更があれば接続の直後に初回の保存を作る
pub(crate) async fn connect_flow(
    app: tauri::AppHandle,
    state: &AppState,
    id: &str,
    request: RemoteRequest,
    first_save: bool,
) -> Result<RemoteConnectResult, AppError> {
    check_public_confirmed(&request)?;

    // 状態の確認（読み取りのみ）。すでに保存先がある、フォルダが無い場合は何も作らない
    let store = state.store_clone();
    let lookup_id = id.to_string();
    let (project, origin) = run_blocking(move || {
        let project = load_project(&store, &lookup_id)?;
        if core_ops::project_folder_state(&project.path).is_missing() {
            return Err(error(
                "project_folder_missing",
                "プロジェクトのフォルダが見つかりません。",
                NOTHING_CREATED,
                "フォルダの場所を指定してから、もう一度お試しください",
                None,
            ));
        }
        let origin = core_ops::Ops::new(crate::git_runner())
            .origin_url(&project.path)
            .map_err(AppError::from_ops_error)?;
        Ok((project, origin))
    })
    .await?;
    let already = project.remote_url.as_deref().is_some_and(|u| !u.is_empty());
    if already || origin.is_some() {
        return Err(error(
            "remote_already_connected",
            "このプロジェクトは、すでにクラウドの保管場所に接続されています。",
            "ファイルは変更されていません。GitHub には何も作られていません。",
            "画面を更新して、状態を確認してください",
            None,
        ));
    }

    // (1) GitHub にリポジトリを作る。トークンはここでだけ使う
    let token = require_token().await?;
    let user = current_user(state, &token)
        .await
        .map_err(|e| create_error(state, e))?;
    let (owner_login, kind) = if request.owner.eq_ignore_ascii_case(&user.login) {
        (user.login.clone(), OwnerKind::Personal)
    } else {
        (request.owner.clone(), OwnerKind::Org)
    };
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| suggest_repository_name(&project.display_name, id));
    let repo = GithubApi::new()
        .create_repository(&token, &owner_login, kind, &name, request.private, None)
        .await
        .map_err(|e| create_error(state, e))?;
    drop(token);

    // 取得先は常に https://github.com/<owner>/<name>.git（認証情報を含まない）
    let full_name = repo.full_name.clone();
    let Some(url) = clone_url_for(&full_name) else {
        return Err(auth_error(AuthError::UnexpectedStatus, FILES_SAFE));
    };

    // (2)(3) 保存先の設定と初回の保存。直列キューとジャーナルを通す
    let ctx = OpContext::new(app, state);
    let limits = size_limits_for(&ctx.store, id);
    let connect_store = ctx.store.clone();
    let connect_id = id.to_string();
    let stored_owner = repo.owner_login.clone();
    let connected = run_op(
        &ctx,
        id,
        OpSpec {
            operation: "connect-remote",
            trigger: crate::events::OpTrigger::Manual,
            target: Some(full_name.clone()),
            data_is_safe: "ファイルは変更されていません",
        },
        move |ops, path| {
            // 初回の保存は、まだ 1 つも保存が無いプロジェクトだけ（既存の履歴は勝手に増やさない）
            let memo = if first_save && ops.commit_count(path)? == 0 {
                Some(FIRST_SAVE_MEMO)
            } else {
                None
            };
            let outcome = ops.connect(path, &url, memo, limits)?;
            let guard = connect_store
                .lock()
                .map_err(|_| OpsError::Unexpected("database lock failed".to_string()))?;
            guard
                .update_project(
                    &connect_id,
                    None,
                    Some(Some(url.as_str())),
                    Some(stored_owner.as_str()),
                    None,
                )
                .map_err(|e| OpsError::Unexpected(format!("{e:?}")))?;
            drop(guard);
            // 初回の保存を作った場合に、付け替え照合用の OID を記録する
            record_initial_commit(&connect_store, &connect_id, path, ops);
            Ok(outcome)
        },
        |o| OpSummary {
            detail: match o.first_save {
                FirstSave::Saved => "connected-first-saved",
                FirstSave::NeedsSizeDecision(_) => "connected-needs-size-decision",
                FirstSave::NothingToSave | FirstSave::NotRequested => "connected",
            }
            .to_string(),
            quiet: false,
        },
    )
    .await;
    let connected = match connected {
        Ok(outcome) => outcome,
        Err(failure) => {
            // GitHub 上のリポジトリは作成済みだが、この PC のプロジェクトに結び付けられなかった
            let technical = match &failure {
                OpFailure::Ops(e) => redact(&format!("{e:?}")),
                OpFailure::App(e) => e.technical_info.clone().unwrap_or_default(),
            };
            return Err(error(
                "remote_connect_failed",
                "GitHub に保存先は作成しましたが、このプロジェクトに接続できませんでした。",
                "ファイルはこの PC に安全に残っています。",
                "もう一度お試しください。同じ名前は使えないため、別の名前を指定してください",
                Some(technical),
            ));
        }
    };
    // 取り込み・アップロードの計画に加える
    state.scheduler.refresh();

    let skipped = match connected.first_save {
        FirstSave::NeedsSizeDecision(found) => Some(size_check_result(found)),
        _ => None,
    };

    // (4) 初回のアップロード（`push -u origin HEAD`）。通常のアップロードと同じ経路
    let pushed = run_op(
        &ctx,
        id,
        OpSpec {
            operation: "push",
            trigger: crate::events::OpTrigger::Manual,
            target: None,
            data_is_safe: "ファイルは安全です",
        },
        move |ops, path| ops.upload_with(path, limits),
        summarize_push,
    )
    .await;
    state
        .scheduler
        .note_result(id, SyncTask::Push, push_task_result(&pushed));

    let mut result = RemoteConnectResult {
        repository: Some(full_name),
        connected: true,
        uploaded: false,
        size_check: skipped,
        error: None,
    };
    match pushed {
        Ok(UploadOutcome::Pushed | UploadOutcome::PulledThenPushed(_)) => result.uploaded = true,
        Ok(UploadOutcome::NeedsSizeDecision(found)) => {
            result.size_check = Some(size_check_result(found));
        }
        Ok(UploadOutcome::NothingToUpload | UploadOutcome::NeedsResolve(_)) => {}
        Err(failure) => result.error = Some(upload_error(failure)),
    }
    Ok(result)
}

/// ローカルだけのプロジェクトを、GitHub 上の新しいリポジトリに接続する。
///
/// 新規作成のときと同じ流れだが、初回の保存は作らない（保存は利用者が行う）。すでにある保存は
/// 初回のアップロードで上がる。同名のリポジトリが GitHub にすでにある場合は `remote_name_taken`
/// （何も作られず、ローカルも変更されない）。
#[tauri::command]
#[specta::specta]
pub async fn connect_remote(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    request: RemoteRequest,
) -> Result<RemoteConnectResult, AppError> {
    connect_flow(app, &state, &id, request, false).await
}
