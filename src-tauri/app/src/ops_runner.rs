// 状態変更操作の共通枠。
//
// 直列キュー（`run_exclusive`、安全上の不変条件 7）の内側で操作を実行し、
// 前後の復元点の差からジャーナルに「どの復元点を作ったか」を記録し、UI へイベントを通知する。
// 手動のコマンドとスケジューラ（自動実行）の両方がこの関数を通る。

use std::path::Path;
use std::sync::{Arc, Mutex};

use core_git::GitError;
use core_ops::{new_restore_points, Labels, MemoLabels, Ops, OpsError, PullOutcome, UploadOutcome};
use core_store::{
    now_rfc3339, JournalOutcome, JournalTrigger, NewJournalEntry, ProjectLocks, Store,
};
use core_watch::{classify_failure, FailureKind};
use tauri_specta::Event;

use crate::events::{OpFinished, OpProgress, OpTrigger, StatusChanged};
use crate::{run_exclusive, AppError, AppState};

/// 操作の実行コンテキスト（コマンドとスケジューラで共有する）
#[derive(Clone)]
pub(crate) struct OpContext {
    pub app: tauri::AppHandle,
    pub store: Arc<Mutex<Store>>,
    pub locks: Arc<ProjectLocks>,
}

impl OpContext {
    pub fn new(app: tauri::AppHandle, state: &AppState) -> Self {
        OpContext {
            app,
            store: state.store_clone(),
            locks: state.locks_clone(),
        }
    }
}

/// 操作の種類・起動元・失敗時の説明
pub(crate) struct OpSpec {
    /// `save` / `restore` / `pull` / `push` / `resolve`
    pub operation: &'static str,
    pub trigger: OpTrigger,
    /// ジャーナルに残す対象（元に戻す先のコミットなど）
    pub target: Option<String>,
    /// キューやロックで失敗したときに「データは無事か」へ出す文言
    pub data_is_safe: &'static str,
}

/// 成功した操作の要約
pub(crate) struct OpSummary {
    /// 結果の要約（`merged` など）。エラー本文は入れない
    pub detail: String,
    /// 自動実行で何も起きなかった場合に true（ジャーナル・状態通知を省く）
    pub quiet: bool,
}

/// 操作の失敗。操作自体の失敗と、キュー・DB など基盤側の失敗を区別する。
pub(crate) enum OpFailure {
    Ops(OpsError),
    App(AppError),
}

impl From<OpFailure> for AppError {
    fn from(f: OpFailure) -> Self {
        match f {
            OpFailure::Ops(e) => AppError::from_ops_error(e),
            OpFailure::App(e) => e,
        }
    }
}

/// キューの内側で得た結果
struct Done<T> {
    result: Result<T, OpsError>,
    detail: String,
    quiet: bool,
}

/// `OpsError` を同期の失敗種別へ分類する（オフライン・認証失敗・ぶつかり・その他）。
/// 判定に使うのは git の stderr だが、外へ出すのは分類結果だけ。
pub(crate) fn failure_kind(e: &OpsError) -> FailureKind {
    match e {
        OpsError::Conflict(_) => FailureKind::Conflict,
        OpsError::Git(GitError::Timeout { .. }) => FailureKind::Offline,
        OpsError::Git(GitError::Failed { stderr, .. }) => classify_failure(stderr),
        _ => FailureKind::Other,
    }
}

/// 状態変更操作を直列キューで実行する。
///
/// - 実行は必ず `run_exclusive`（同一プロジェクトは 1 件ずつ）
/// - 復元点は `body` の中（core-ops）が作る。ここでは前後の差を記録するだけ
/// - ジャーナルの記録失敗は操作の成否に影響させない
pub(crate) async fn run_op<T, B, S>(
    ctx: &OpContext,
    id: &str,
    spec: OpSpec,
    body: B,
    summarize: S,
) -> Result<T, OpFailure>
where
    T: Send + 'static,
    B: FnOnce(&Ops, &Path) -> Result<T, OpsError> + Send + 'static,
    S: FnOnce(&T) -> OpSummary + Send + 'static,
{
    let operation = spec.operation;
    let trigger = spec.trigger;
    let project_id = id.to_string();

    let _ = OpProgress {
        project_id: project_id.clone(),
        operation: operation.to_string(),
        trigger,
    }
    .emit(&ctx.app);

    let store = ctx.store.clone();
    let target = spec.target.clone();
    let id_owned = project_id.clone();

    let outcome = run_exclusive(
        ctx.locks.clone(),
        project_id.clone(),
        spec.data_is_safe,
        move || {
            let project = {
                let guard = store.lock().map_err(|e| AppError {
                    what_happened: "データベースアクセスに失敗しました".to_string(),
                    data_is_safe: "ファイルは変更されていません".to_string(),
                    next_action: "もう一度試してください".to_string(),
                    technical_info: Some(e.to_string()),
                })?;
                guard.get_project(&id_owned).map_err(|e| AppError {
                    what_happened: "プロジェクトが見つかりません".to_string(),
                    data_is_safe: "何も変更されていません".to_string(),
                    next_action: "プロジェクト一覧から確認してください".to_string(),
                    technical_info: Some(format!("{:?}", e)),
                })?
            };

            let ops = Ops::new(crate::git_runner()).with_labels(app_labels());
            let started_at = now_rfc3339();
            let before = ops.restore_point_refs(&project.path).unwrap_or_default();

            let result = body(&ops, &project.path);

            let after = ops.restore_point_refs(&project.path).unwrap_or_default();
            let restore = new_restore_points(&before, &after);

            let auto = trigger == OpTrigger::Auto;
            let (journal_outcome, detail, quiet) = match &result {
                Ok(value) => {
                    let s = summarize(value);
                    (JournalOutcome::Success, s.detail, auto && s.quiet)
                }
                Err(e) => (
                    JournalOutcome::Failure,
                    e.kind().to_string(),
                    auto && failure_kind(e) == FailureKind::Offline,
                ),
            };

            if !quiet {
                let entry = NewJournalEntry {
                    project_id: id_owned.clone(),
                    operation: operation.to_string(),
                    trigger: if auto {
                        JournalTrigger::Auto
                    } else {
                        JournalTrigger::Manual
                    },
                    started_at,
                    finished_at: now_rfc3339(),
                    outcome: journal_outcome,
                    detail: Some(detail.clone()),
                    snapshot_ref: restore.snapshot_ref,
                    backup_ref: restore.backup_ref,
                    target,
                };
                // 記録に失敗しても操作の結果は変えない
                let recorded = store
                    .lock()
                    .ok()
                    .is_some_and(|g| g.record_journal(&entry).is_ok());
                if !recorded {
                    eprintln!("操作ジャーナルの記録に失敗しました（操作: {operation}）");
                }
            }

            Ok(Done {
                result,
                detail,
                quiet,
            })
        },
    )
    .await;

    match outcome {
        Ok(done) => {
            let _ = OpFinished {
                project_id: project_id.clone(),
                operation: operation.to_string(),
                trigger,
                ok: done.result.is_ok(),
                outcome: done.detail.clone(),
            }
            .emit(&ctx.app);
            if !done.quiet {
                let _ = StatusChanged { project_id }.emit(&ctx.app);
            }
            done.result.map_err(OpFailure::Ops)
        }
        Err(app_error) => {
            let _ = OpFinished {
                project_id,
                operation: operation.to_string(),
                trigger,
                ok: false,
                outcome: "internal".to_string(),
            }
            .emit(&ctx.app);
            Err(OpFailure::App(app_error))
        }
    }
}

/// 履歴に残る文言（日本語）。取り込み前の自動保存のメモと、別名コピーのファイル名に使う。
pub(crate) fn app_labels() -> Labels {
    Labels {
        auto_save_memo: "取り込み前の自動保存".to_string(),
        copy_mine: "この PC の版".to_string(),
        copy_theirs: "クラウドの版".to_string(),
    }
}

/// 保存メモの案に使う文言（設計書 10.3）
pub(crate) fn memo_labels() -> MemoLabels {
    MemoLabels {
        added_one: "{name} を追加".to_string(),
        modified_one: "{name} を更新".to_string(),
        deleted_one: "{name} を削除".to_string(),
        renamed_one: "{old} を {new} に名前変更".to_string(),
        added_group: "{category}を{count}件追加".to_string(),
        modified_group: "{category}を{count}件更新".to_string(),
        deleted_group: "{category}を{count}件削除".to_string(),
        renamed_group: "{category}を{count}件名前変更".to_string(),
        mixed: "{name} ほか {count} 件".to_string(),
        category_image: "画像".to_string(),
        category_document: "文書".to_string(),
        category_other: "ファイル".to_string(),
    }
}

/// 取り込み結果の名前（UI への返却値とジャーナルで共通）
pub(crate) fn pull_outcome_name(o: &PullOutcome) -> &'static str {
    match o {
        PullOutcome::UpToDate => "up-to-date",
        PullOutcome::FastForwarded => "fast-forwarded",
        PullOutcome::Merged { .. } => "merged",
        PullOutcome::Conflicted { .. } => "conflicted",
        PullOutcome::NoUpstream => "no-upstream",
    }
}

/// アップロード結果の名前（UI への返却値とジャーナルで共通）
pub(crate) fn push_outcome_name(o: &UploadOutcome) -> &'static str {
    match o {
        UploadOutcome::Pushed => "pushed",
        UploadOutcome::NothingToUpload => "nothing",
        UploadOutcome::PulledThenPushed(_) => "pulled-then-pushed",
        UploadOutcome::NeedsResolve(_) => "needs-resolve",
    }
}

pub(crate) fn summarize_pull(o: &PullOutcome) -> OpSummary {
    OpSummary {
        detail: pull_outcome_name(o).to_string(),
        quiet: matches!(o, PullOutcome::UpToDate | PullOutcome::NoUpstream),
    }
}

pub(crate) fn summarize_push(o: &UploadOutcome) -> OpSummary {
    OpSummary {
        detail: push_outcome_name(o).to_string(),
        quiet: matches!(o, UploadOutcome::NothingToUpload),
    }
}
