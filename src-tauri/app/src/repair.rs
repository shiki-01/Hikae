// 破損リポジトリの確認付き復旧（設計書 5.2 E10「取得し直す」）。
//
// 手順（設計書 5.2 E10 の 1〜4）: 監視を止める → 作業フォルダを `<名前>_退避_<日時>` へ移動 →
// 元のパスへ保存先から clone（github.rs の `clone_into`）→ 退避フォルダの未保存の内容をコピーし直す。
// clone に失敗したら退避フォルダを元の名前へ戻す（`restore_aside`）。
//
// - **削除はしない。** ファイルの移動（rename）・コピー・clone だけを行う。退避フォルダは自動削除しない
// - **復元点は作らない。** 壊れたリポジトリには復元点を作れないため、移動した退避フォルダが復元点の代わりになる
//   （作業フォルダの内容は一切変更・削除しない。不変条件 3・6）
// - 前提（再検査でも破損のまま・保存先が設定済み・GitHub にログイン済み）を 1 つでも満たさなければ、
//   何も変更せずに理由（`RepairResult::reason`）を返す
// - 状態を変える操作なので、同一プロジェクトの直列キュー（`run_exclusive`）の中で行う（不変条件 7）
// - 操作ジャーナルに操作名 `repair` で開始と結果を残す。復旧の対象（`is_recoverable_operation`）には入れない
//   （復元点が無いため、中断の自動確定や「元に戻す」の対象にしない）
// - git は `GitRunner`（`git_runner()`）経由でのみ呼ぶ。clone の固定 URL は `clone_url_for` の形だけを受け付ける

use std::path::Path;
use std::sync::{Arc, Mutex};

use core_github::clone_url_for;
use core_ops::{copy_back_unsaved, move_aside, restore_aside, Identity, Ops};
use core_store::{
    now_rfc3339, JournalFinish, JournalOutcome, JournalTrigger, RunningJournal, Store,
};
use tauri_specta::Event;

use crate::events::StatusChanged;
use crate::github::{clone_into, current_user, load_token};
use crate::health::ProjectHealth;
use crate::scheduler::SchedulerHandle;
use crate::{run_exclusive, AppError, AppState};

/// ジャーナルの操作名（設計書 5.2 E10）
const JOURNAL_OPERATION: &str = "repair";

/// 状態を変えない操作の共通の「データは無事」の文言（直列キューの失敗時にも使う）
const DATA_SAFE: &str = "ファイルは変更されていません。";

/// 復旧の結果。前提を満たさず何もしなかったときは `reason` に理由を入れる。
/// 理由は `not_broken`（再検査で壊れていない）、`no_remote`（保存先が無い）、
/// `remote_unsupported`（保存先が GitHub の固定の形ではない）、`not_logged_in`、`folder_missing`（フォルダが無い）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RepairResult {
    /// 何もしなかった理由（復旧できたときは None）
    pub reason: Option<String>,
    /// 退避フォルダのパス（復旧したときだけ）。利用者が中身を確かめて、必要なら手で戻せる
    pub aside_path: Option<String>,
    /// 新しいフォルダへコピーし直したファイルの数
    pub copied: u32,
    /// コピーできなかったファイルの相対パス（退避フォルダに残っている）
    pub failed: Vec<String>,
}

impl RepairResult {
    /// 前提を満たさず、何も変更しなかった
    fn skipped(reason: &str) -> Self {
        RepairResult {
            reason: Some(reason.to_string()),
            aside_path: None,
            copied: 0,
            failed: Vec::new(),
        }
    }
}

/// 確認付きの復旧（設計書 5.2 E10）。画面の「取得し直す」から呼ぶ。
///
/// 読み取り（ログインの確認）だけは直列キューの外で行い、状態を変える部分は直列キューの中で行う。
#[tauri::command]
#[specta::specta]
pub(crate) async fn repair_broken_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> Result<RepairResult, AppError> {
    // ログインの確認（読み取りのみ）。未ログインなら、後で理由として返す
    let identity = match load_token().await? {
        None => None,
        Some(token) => {
            let user = current_user(&state, &token)
                .await
                .map_err(|e| crate::github::auth_error(e, DATA_SAFE))?;
            Some(Identity {
                name: user.login.clone(),
                email: user.noreply_email(),
            })
        }
    };

    let store = state.store_clone();
    let health = state.health.clone();
    let scheduler = state.scheduler.clone();
    let id = project_id.clone();
    let result = run_exclusive(
        state.locks_clone(),
        project_id.clone(),
        DATA_SAFE,
        move || repair_locked(&store, &health, &scheduler, &id, identity.as_ref()),
    )
    .await;

    // 結果によらず、監視と実行計画を登録の内容に合わせ直す（止めたままにしない）
    state.scheduler.refresh();
    let _ = StatusChanged {
        project_id: project_id.clone(),
    }
    .emit(&app);
    result
}

/// 直列キューの中での復旧本体
fn repair_locked(
    store: &Arc<Mutex<Store>>,
    health: &ProjectHealth,
    scheduler: &SchedulerHandle,
    id: &str,
    identity: Option<&Identity>,
) -> Result<RepairResult, AppError> {
    let project = crate::load_project(store, id)?;
    let ops = Ops::new(crate::git_runner());

    // 前提 (a): 再検査でも破損のまま（フォルダの有無、軽い検査の結果）
    if core_ops::project_folder_state(&project.path).is_missing() {
        return Ok(RepairResult::skipped("folder_missing"));
    }
    let expect_commits = project.initial_commit.is_some();
    let health_now = ops
        .check_repo_health(&project.path, expect_commits)
        .map_err(AppError::from_ops_error)?;
    if !health_now.is_broken() {
        return Ok(RepairResult::skipped("not_broken"));
    }

    // 前提 (b): 保存先が設定済みで、固定の形（clone_url_for）の URL であること
    let Some(remote) = project.remote_url.as_deref().filter(|u| !u.is_empty()) else {
        return Ok(RepairResult::skipped("no_remote"));
    };
    let Some(url) = fixed_clone_url(remote) else {
        return Ok(RepairResult::skipped("remote_unsupported"));
    };

    // 前提 (c): GitHub にログイン済み
    let Some(identity) = identity else {
        return Ok(RepairResult::skipped("not_logged_in"));
    };

    // 操作の開始を記録する。ここで失敗したら、何も変えずに返す
    let journal_id = {
        let guard = store.lock().map_err(|_| crate::github::lock_error())?;
        guard
            .begin_journal(&RunningJournal {
                project_id: id.to_string(),
                operation: JOURNAL_OPERATION.to_string(),
                trigger: JournalTrigger::Manual,
                started_at: now_rfc3339(),
                target: None,
            })
            .map_err(crate::github::store_error)?
    };

    // 監視と実行計画を止める（破損の記憶を立てているので、巡回も監視を開き直さない）
    health.set_broken(id, true);
    scheduler.stop_watch(id);
    scheduler.forget(id);

    // 手順 1: 作業フォルダを退避フォルダへ移動する（rename のみ）
    let aside = match move_aside(&project.path, &aside_stamp()) {
        Ok(aside) => aside,
        Err(e) => {
            finish_journal(store, journal_id, JournalOutcome::Failure, "move_failed");
            return Err(AppError::from_ops_error(e));
        }
    };

    // 手順 2: 元のパスへ保存先から取得する。失敗したら退避フォルダを元の名前へ戻す（削除はしない）
    if let Err(clone_err) = clone_into(&project.path, &url, identity) {
        return match restore_aside(&aside, &project.path) {
            Ok(()) => {
                finish_journal(store, journal_id, JournalOutcome::Failure, "clone_failed");
                Err(clone_err)
            }
            Err(restore_err) => {
                finish_journal(store, journal_id, JournalOutcome::Failure, "restore_failed");
                Err(restore_failed_error(&aside, &restore_err))
            }
        };
    }

    // 手順 3: 退避フォルダの未保存の内容を、新しいフォルダへコピーし直す（退避フォルダは変えない）
    let report = match copy_back_unsaved(&aside, &project.path) {
        Ok(report) => report,
        Err(copy_err) => {
            // 取得は済んでいて、リポジトリは読める。記憶を捨てて、退避フォルダの場所を知らせる
            health.forget(id);
            finish_journal(store, journal_id, JournalOutcome::Failure, "copy_failed");
            return Err(copy_failed_error(&aside, &copy_err));
        }
    };

    // 手順 4: 破損の記憶を解除する（次の巡回で監視と実行計画を開き直す）
    health.forget(id);
    finish_journal(
        store,
        journal_id,
        JournalOutcome::Success,
        &format!("copied={} failed={}", report.copied, report.failed.len()),
    );

    Ok(RepairResult {
        reason: None,
        aside_path: Some(aside.to_string_lossy().into_owned()),
        copied: report.copied,
        failed: report.failed,
    })
}

/// 保存先の URL（`https://github.com/<owner>/<name>.git`）から、`clone_url_for` の固定の形を作り直す。
/// 同じ形でなければ None（任意の URL の取得はしない）
fn fixed_clone_url(remote: &str) -> Option<String> {
    let full_name = remote
        .strip_prefix("https://github.com/")?
        .strip_suffix(".git")?;
    let url = clone_url_for(full_name)?;
    (url == remote).then_some(url)
}

/// 退避フォルダの名前に使う時刻（`YYYYMMDD-HHMMSS`、UTC）。`move_aside` が形式を検査する
fn aside_stamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// 操作の結果をジャーナルに書く。書けなくても復旧の結果は変えない（記録の失敗は画面に出さない）
fn finish_journal(
    store: &Arc<Mutex<Store>>,
    journal_id: i64,
    outcome: JournalOutcome,
    detail: &str,
) {
    if let Ok(guard) = store.lock() {
        let _ = guard.finish_journal(
            journal_id,
            &JournalFinish {
                finished_at: now_rfc3339(),
                outcome,
                detail: Some(detail.to_string()),
                snapshot_ref: None,
                backup_ref: None,
            },
        );
    }
}

/// 元の名前へ戻せなかったときのエラー。退避フォルダの場所を必ず示す
fn restore_failed_error(aside: &Path, e: &core_ops::OpsError) -> AppError {
    let path = aside.to_string_lossy().into_owned();
    AppError {
        code: "repair_restore_failed".to_string(),
        params: vec![("path".to_string(), path.clone())],
        what_happened: "取得し直しに失敗し、元のフォルダへ戻せませんでした".to_string(),
        data_is_safe: format!("ファイルは次のフォルダに残っています: {path}"),
        next_action: "そのフォルダの場所を確認してください".to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}

/// 未保存の内容をコピーし直せなかったときのエラー。取得自体は済んでいる
fn copy_failed_error(aside: &Path, e: &core_ops::OpsError) -> AppError {
    let path = aside.to_string_lossy().into_owned();
    AppError {
        code: "repair_copy_failed".to_string(),
        params: vec![("path".to_string(), path.clone())],
        what_happened: "取得し直しはできましたが、未保存の内容を戻せませんでした".to_string(),
        data_is_safe: format!("未保存の内容は次のフォルダに残っています: {path}"),
        next_action: "そのフォルダから必要なファイルを手で戻してください".to_string(),
        technical_info: Some(format!("{e:?}")),
    }
}
