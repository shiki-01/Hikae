// 保存先（GitHub 上のリポジトリ）の作成と接続（設計書 4.6）。
//
// 流れ: (0) ローカル側の検査（`Ops::preflight_connect`。読み取りのみ）→ (1) GitHub にリポジトリを作る
//       → (2) `origin` を設定する（`run_op`、操作名 `connect-remote`）
//       → (3) 初回の保存（頼まれたときだけ）→ (4) 初回のアップロード（`run_op`、操作名 `push`）。
//
// (0) は GitHub に何も作る前に、`origin` が別の場所を指していないか、`.git/config` が書き込めるか、
// フォルダが使えるか、初回の保存に大きいファイルが無いかを確かめる。問題があれば何も作らずに断る。
// 同名の空の保存先が GitHub にあるときは、作らずに接続を提案する（`adopt_existing` で承認を受けてから
// 接続する。空でない保存先には決して接続しない）。(2)(3) の失敗で GitHub に空の保存先が残る場合は
// `remote_orphaned` で知らせ、もう一度接続すれば同じ保存先に接続できる。
//
// 安全上の不変条件との対応:
// - 2・3: git は `GitRunner` 経由のみ。`origin` の設定は `.git/config` だけを変え、初回の保存は通常の
//   保存と同じ復元点を作る。アップロードは `push -u origin HEAD`（force 系は許可リストが拒否する）
// - 7: (2) と (4) は `run_op`（= `run_exclusive`）の中で実行する。GitHub の API 呼び出し (1) は
//   作業フォルダにもインデックスにも触れないため、キューの外で行う
// - 9: `origin` の URL は `https://github.com/<owner>/<name>.git` で認証情報を含めない（認証は credential
//   helper）。トークンは API 呼び出しにだけ使い、エラー・ジャーナル・params・イベントには出さない
// - 公開（private=false）は、画面の警告に同意した明示のフラグ（`public_confirmed`）があるときだけ許可する

use core_github::{
    clone_url_for, suggest_repository_name, validate_owner_login, validate_repository_name,
    AuthError, GithubApi, OwnerKind, RepositoryAdoption,
};
use core_ops::{ConnectPreflight, FirstSave, OpsError, SizeFindings, UploadOutcome};
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
    /// 同名の「空の」保存先がすでにあるとき、新しく作らずにそこへ接続する（利用者が確認したあとだけ true）。
    /// 空でない保存先、書き込めない保存先には接続しない（バックエンドが改めて確かめる）
    pub adopt_existing: bool,
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
    ///
    /// `connected` が false のときは、GitHub には何も作っておらず、ローカルも変更していない
    /// （作成の前の検査で見つかった。画面は 2 択を出し、選択を保存に反映してから接続をやり直す）
    pub size_check: Option<SizeCheckResult>,
    /// 同じ名前の空の保存先（`owner/name`）が GitHub にすでにあり、新しくは作らなかった場合の名前。
    /// 何も作らず、ローカルも変更していない。画面が「ここに接続しますか」と確認し、承認されたら
    /// `adopt_existing` を true にして接続をやり直す。なければ null
    pub existing_empty_repository: Option<String>,
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
            existing_empty_repository: None,
            error: Some(error),
        }
    }

    /// 作成の前に大きいファイルが見つかった（何も作っていない）
    fn needs_size_decision(found: SizeFindings) -> Self {
        RemoteConnectResult {
            size_check: Some(size_check_result(found)),
            ..RemoteConnectResult::not_connected_empty()
        }
    }

    /// 同名の空の保存先がある（何も作っていない）
    fn existing_empty(full_name: String) -> Self {
        RemoteConnectResult {
            existing_empty_repository: Some(full_name),
            ..RemoteConnectResult::not_connected_empty()
        }
    }

    fn not_connected_empty() -> Self {
        RemoteConnectResult {
            repository: None,
            connected: false,
            uploaded: false,
            size_check: None,
            existing_empty_repository: None,
            error: None,
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

/// 既存の空の保存先に接続してよい公開範囲か。非公開を求めたのに既存のものが公開なら、
/// 非公開のつもりのファイルが公開されてしまうため接続しない（公開を求めて既存が非公開なのは問題ない）。
fn adoptable_visibility(request: &RemoteRequest, repo: &core_github::RemoteRepo) -> bool {
    !request.private || repo.private
}

/// 保存先を GitHub に用意したあと、このプロジェクトに接続できなかった失敗（`remote_orphaned`）。
/// `params` に `name`（作成済みの `owner/name`）を入れる。もう一度接続すれば、空のその保存先に
/// 接続できる（同名の空の保存先として提案される）。
fn orphaned_error(full_name: &str, technical: String) -> AppError {
    AppError {
        code: "remote_orphaned".to_string(),
        params: vec![("name".to_string(), full_name.to_string())],
        what_happened: format!(
            "GitHub 上に空の保存先（{full_name}）ができていますが、このプロジェクトには接続できませんでした。"
        ),
        data_is_safe: "ファイルはこの PC に安全に残っています。GitHub 上の保存先も空のままです。"
            .to_string(),
        next_action: "もう一度「接続する」を押してください。同じ名前の空の保存先に接続できます。不要な場合は、GitHub 上で削除してください"
            .to_string(),
        technical_info: Some(technical),
    }
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

    // 接続先の名前を先に決める（通信しない）。名前が規則に合わなければ、何も作らずに断る
    let store = state.store_clone();
    let lookup_id = id.to_string();
    let project = run_blocking(move || load_project(&store, &lookup_id)).await?;
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| suggest_repository_name(&project.display_name, id));
    validate_owner_login(&request.owner).map_err(|e| create_error(state, e))?;
    validate_repository_name(&name).map_err(|e| create_error(state, e))?;
    let planned = format!("{}/{}", request.owner, name);
    let Some(planned_url) = clone_url_for(&planned) else {
        return Err(auth_error(AuthError::UnexpectedStatus, NOTHING_CREATED));
    };

    // (0) GitHub に何かを作る前に、ローカル側で失敗しうる点をすべて確かめる（読み取りのみ）
    if project.remote_url.as_deref().is_some_and(|u| !u.is_empty()) {
        return Err(error(
            "remote_already_connected",
            "このプロジェクトは、すでにクラウドの保管場所に接続されています。",
            "ファイルは変更されていません。GitHub には何も作られていません。",
            "画面を更新して、状態を確認してください",
            None,
        ));
    }
    let preflight_limits = size_limits_for(&state.store_clone(), id);
    let preflight_path = project.path.clone();
    let preflight_url = planned_url.clone();
    let preflight = run_blocking(move || {
        core_ops::Ops::new(crate::git_runner())
            .preflight_connect(
                &preflight_path,
                &preflight_url,
                first_save,
                preflight_limits,
            )
            .map_err(AppError::from_ops_error)
    })
    .await?;
    // `origin` がすでに接続先と同じ場所なら、前回の途中で止まった接続の続き。GitHub には作らない
    let resuming = match preflight {
        ConnectPreflight::Ready => false,
        ConnectPreflight::AlreadyPointsHere => true,
        ConnectPreflight::OriginElsewhere => {
            return Err(error(
                "remote_conflict",
                "このフォルダは、すでに別のクラウドの保管場所につながっています。",
                NOTHING_CREATED,
                "いまのつながりを変えずに使うか、別のフォルダでプロジェクトを作ってください",
                None,
            ));
        }
        ConnectPreflight::ConfigNotWritable => {
            return Err(error(
                "remote_not_writable",
                "このフォルダの設定が書き込めないため、クラウドの保管場所に接続できません。",
                NOTHING_CREATED,
                "フォルダが読み取り専用になっていないか確認してから、もう一度お試しください",
                None,
            ));
        }
        ConnectPreflight::FolderUnavailable(_) => {
            return Err(error(
                "project_folder_missing",
                "プロジェクトのフォルダが見つかりません。",
                NOTHING_CREATED,
                "フォルダの場所を指定してから、もう一度お試しください",
                None,
            ));
        }
        // 大きいファイルは作成の前に知らせる（作っていないので、断っても何も残らない）
        ConnectPreflight::NeedsSizeDecision(found) => {
            return Ok(RemoteConnectResult::needs_size_decision(found));
        }
    };

    // (1) GitHub にリポジトリを作る。トークンはここでだけ使う
    let (full_name, stored_owner, url) = if resuming {
        (planned, request.owner.clone(), planned_url)
    } else {
        let token = require_token().await?;
        let user = current_user(state, &token)
            .await
            .map_err(|e| create_error(state, e))?;
        let (owner_login, kind) = if request.owner.eq_ignore_ascii_case(&user.login) {
            (user.login.clone(), OwnerKind::Personal)
        } else {
            (request.owner.clone(), OwnerKind::Org)
        };
        let api = GithubApi::new();
        let repo = if request.adopt_existing {
            // 承認済みでも、空で書き込めることを改めて確かめる（空でないものには決して接続しない）
            match api
                .check_adoptable_repository(&token, &owner_login, &name)
                .await
            {
                Ok(RepositoryAdoption::Adoptable(repo))
                    if adoptable_visibility(&request, &repo) =>
                {
                    repo
                }
                Ok(_) => return Err(create_error(state, AuthError::RepositoryNameTaken)),
                Err(e) => return Err(create_error(state, e)),
            }
        } else {
            match api
                .create_repository(&token, &owner_login, kind, &name, request.private, None)
                .await
            {
                Ok(repo) => repo,
                // 同名がある。空で書き込めるものだけ、接続を提案する（空でないものは名前の衝突のまま）
                Err(AuthError::RepositoryNameTaken) => {
                    return match api
                        .check_adoptable_repository(&token, &owner_login, &name)
                        .await
                    {
                        Ok(RepositoryAdoption::Adoptable(repo))
                            if adoptable_visibility(&request, &repo) =>
                        {
                            Ok(RemoteConnectResult::existing_empty(repo.full_name))
                        }
                        Ok(_) => Err(create_error(state, AuthError::RepositoryNameTaken)),
                        Err(e) => Err(create_error(state, e)),
                    };
                }
                Err(e) => return Err(create_error(state, e)),
            }
        };
        drop(token);

        // 取得先は常に https://github.com/<owner>/<name>.git（認証情報を含まない）
        let Some(url) = clone_url_for(&repo.full_name) else {
            return Err(auth_error(AuthError::UnexpectedStatus, FILES_SAFE));
        };
        (repo.full_name, repo.owner_login, url)
    };

    // (2)(3) 保存先の設定と初回の保存。直列キューとジャーナルを通す
    let ctx = OpContext::new(app, state);
    let limits = size_limits_for(&ctx.store, id);
    let connect_store = ctx.store.clone();
    let connect_id = id.to_string();
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
            // GitHub 上の保存先は用意できているが、この PC のプロジェクトに結び付けられなかった
            let technical = match &failure {
                OpFailure::Ops(e) => redact(&format!("{e:?}")),
                OpFailure::App(e) => e.technical_info.clone().unwrap_or_default(),
            };
            return Err(orphaned_error(&full_name, technical));
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
        existing_empty_repository: None,
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
/// 初回のアップロードで上がる。同名のリポジトリが GitHub にすでにある場合は、空で書き込めるものだけ
/// `existing_empty_repository` で接続を提案し（何も作られず、ローカルも変更されない）、承認されたら
/// `adopt_existing: true` で呼び直す。空でないものは `remote_name_taken`（接続しない）。
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
