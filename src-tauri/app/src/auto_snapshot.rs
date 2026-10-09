// ファイル監視による自動保存（スナップショット）の実行（設計書 6.2、6.3、7章）。
//
// いつ作るかの判断（静止判定・再試行）は tauri 非依存の `core_watch::SnapshotPlanner`、作り方は
// `core-ops` の `Ops::auto_snapshot`（一時インデックスで作り、作業フォルダ・インデックスを変更しない）。
// ここは同一プロジェクトへの状態変更と同じ直列キュー（`run_exclusive`。不変条件 7）の内側で実行し、
// 結果を操作ジャーナルへ記録し、UI へ通知するだけの薄い層。
//
// `run_op` は使わない: `run_op` は開始時に「実行中」の行を作るため、実行中にアプリが終了すると
// 次の起動で「中断された操作」（E15）として利用者に案内してしまう。自動保存は作業フォルダを
// 変更しないので、中断されても案内の必要がない。完了してから結果を 1 行記録する。
// 何も作らなかった（変更なし・同じ内容）試行と、ぶつかり解消中の見送りは記録に残さない。

use core_ops::{AutoSnapshotOutcome, Ops, OpsError};
use core_store::{now_rfc3339, JournalOutcome, JournalTrigger, NewJournalEntry, Project, Store};
use core_watch::SnapshotAttempt;
use std::sync::{Arc, Mutex};
use tauri_specta::Event;

use crate::events::StatusChanged;
use crate::ops_runner::{size_limits_for, OpContext};
use crate::run_exclusive;

/// ジャーナルの操作名
pub(crate) const OPERATION: &str = "auto-snapshot";

/// 自動保存を試みる。結果は計画（`SnapshotPlanner`）へ渡す形で返す。
pub(crate) async fn run(ctx: &OpContext, project: &Project) -> SnapshotAttempt {
    let store = ctx.store.clone();
    let signing = ctx.signing_user();
    let limits = size_limits_for(&ctx.store, &project.id);
    let path = project.path.clone();
    let id = project.id.clone();

    let outcome = run_exclusive(
        ctx.locks.clone(),
        project.id.clone(),
        "ファイルは安全です",
        move || {
            // 順番を待つあいだに「一覧から外す」が実行されていたら、何も作らない
            let registered = store
                .lock()
                .ok()
                .is_some_and(|g| g.get_project(&id).is_ok());
            if !registered {
                return Ok(Ok(AutoSnapshotOutcome::Unchanged));
            }
            let ops = Ops::new(crate::git_runner())
                .with_signing_user(signing.as_ref().map(|(id, login)| (*id, login.as_str())));
            let started_at = now_rfc3339();
            let result = ops.auto_snapshot(&path, limits);
            record(&store, &id, started_at, &result);
            Ok(result)
        },
    )
    .await;

    match outcome {
        Ok(Ok(AutoSnapshotOutcome::Created { .. })) => {
            // 履歴（自動保存の一覧）と「最後の自動保存」を取り直させる
            let _ = StatusChanged {
                project_id: project.id.clone(),
            }
            .emit(&ctx.app);
            SnapshotAttempt::Completed
        }
        Ok(Ok(AutoSnapshotOutcome::Unchanged)) => SnapshotAttempt::Completed,
        Ok(Ok(AutoSnapshotOutcome::Skipped(_))) => SnapshotAttempt::Deferred,
        Ok(Err(_)) | Err(_) => SnapshotAttempt::Failed,
    }
}

/// 結果をジャーナルへ記録する（記録に失敗しても自動保存の成否は変えない）。
/// 失敗の `detail` には種類名だけを入れ、エラー本文は入れない（不変条件 9）。
fn record(
    store: &Arc<Mutex<Store>>,
    project_id: &str,
    started_at: String,
    result: &Result<AutoSnapshotOutcome, OpsError>,
) {
    let (outcome, detail, snapshot_ref) = match result {
        Ok(AutoSnapshotOutcome::Created {
            snapshot_ref,
            excluded_large,
        }) => {
            let detail = if excluded_large.is_empty() {
                "created".to_string()
            } else {
                // 大きいファイルは自動保存に含めない。含めなかった件数を残す
                format!("created-without-large-files:{}", excluded_large.len())
            };
            (JournalOutcome::Success, detail, Some(snapshot_ref.clone()))
        }
        Ok(_) => return,
        Err(e) => (JournalOutcome::Failure, e.kind().to_string(), None),
    };
    let recorded = store.lock().ok().is_some_and(|g| {
        g.record_journal(&NewJournalEntry {
            project_id: project_id.to_string(),
            operation: OPERATION.to_string(),
            trigger: JournalTrigger::Auto,
            started_at,
            finished_at: now_rfc3339(),
            outcome,
            detail: Some(detail),
            snapshot_ref,
            backup_ref: None,
            target: None,
        })
        .is_ok()
    });
    if !recorded {
        eprintln!("操作ジャーナルの記録に失敗しました（操作: {OPERATION}）");
    }
}
