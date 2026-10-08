// 保守作業（起動時と日次、スケジューラから呼ぶ）。
//
// - 復元点（`refs/hikae/snapshots/` と `refs/hikae/backup/`）の間引き（設計書 6.2）
// - 保持期間を過ぎた操作ジャーナルの整理（設計書 6.3、7章の保持期間）
//
// 実行は `run_exclusive`（同一プロジェクトへの状態変更と同じ直列キュー）を通す。
// 間引きは `core-safety` の `thin_restore_points`（`update-ref -d` で `refs/hikae/` 配下の
// 復元点だけを削除）で行い、通常のブランチ・タグ・作業フォルダ・インデックスには触れない。

use std::collections::HashSet;

use core_safety::{thin_restore_points, ThinPolicy, BACKUP_RETENTION_DAYS};
use core_store::{journal_cutoff_from_now, AppSettings, Project};
use tauri_specta::Event;
use time::OffsetDateTime;

use crate::events::StatusChanged;
use crate::ops_runner::OpContext;
use crate::run_exclusive;

/// すべての登録プロジェクトを保守する。1 件の失敗で他のプロジェクトを止めない。
pub(crate) async fn run(ctx: &OpContext, projects: &[(Project, AppSettings)]) {
    for (project, settings) in projects {
        maintain_project(ctx, project, settings).await;
    }

    // 登録が外されたプロジェクトの古い記録も整理する（復元点は触らない）
    let store = ctx.store.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(guard) = store.lock() {
            let _ = guard.purge_journal(None, &journal_cutoff_from_now(BACKUP_RETENTION_DAYS));
        }
    })
    .await;
}

async fn maintain_project(ctx: &OpContext, project: &Project, settings: &AppSettings) {
    let store = ctx.store.clone();
    let id = project.id.clone();
    let path = project.path.clone();
    let snapshot_days = settings.snapshot_retention_days;

    let outcome = run_exclusive(
        ctx.locks.clone(),
        project.id.clone(),
        "ファイルは安全です",
        move || {
            let mut deleted = 0usize;

            // 利用者の手動の操作に紐づく復元点は間引かない。保護対象を読めないときは間引きを見送る
            let protected: Option<HashSet<String>> = store
                .lock()
                .ok()
                .and_then(|g| g.manual_restore_refs(&id).ok())
                .map(|refs| refs.into_iter().collect());

            // フォルダが見つからない（E11）プロジェクトは間引きだけ見送り、記録の整理は行う
            if let (Some(protected), true) = (protected, path.is_dir()) {
                let policy = ThinPolicy {
                    snapshot_retention_days: snapshot_days,
                    backup_retention_days: BACKUP_RETENTION_DAYS,
                };
                if let Ok(report) = thin_restore_points(
                    &crate::git_runner(),
                    &path,
                    OffsetDateTime::now_utc(),
                    &policy,
                    &protected,
                ) {
                    deleted = report.deleted.len();
                    // 消えた復元点を、ジャーナルが指したままにしない
                    if let Ok(guard) = store.lock() {
                        let _ = guard.forget_restore_refs(&id, &report.deleted);
                    }
                }
            }

            // ジャーナルは、復元点より先に消えないよう backup の保持期間（90 日）以上は残す
            let journal_days = snapshot_days.max(BACKUP_RETENTION_DAYS);
            if let Ok(guard) = store.lock() {
                let _ = guard.purge_journal(Some(&id), &journal_cutoff_from_now(journal_days));
            }
            Ok(deleted)
        },
    )
    .await;

    // 履歴の自動保存の一覧が変わるため、画面に再取得させる
    if matches!(outcome, Ok(n) if n > 0) {
        let _ = StatusChanged {
            project_id: project.id.clone(),
        }
        .emit(&ctx.app);
    }
}
