// 保守作業（起動時と日次、スケジューラから呼ぶ）。
//
// - 復元点（`refs/hikae/snapshots/` と `refs/hikae/backup/`）の間引き（設計書 6.2）
// - 保持期間を過ぎた操作ジャーナルの整理（設計書 6.3、7章の保持期間）
// - 既存のプロジェクトへの、OS・Office の一時ファイルの除外設定（`.git/info/exclude`）の適用。
//   管理する部分がまだ無いプロジェクトに 1 回だけ。変更の前に復元点を作り、作業フォルダのファイルは
//   変更しない（設計書 13.1）。`run_op` ではなく `run_exclusive` と結果の記録だけを使う
//   （実行中にアプリが終了しても、中断された操作（E15）として利用者に案内しないため）
//
// 実行は `run_exclusive`（同一プロジェクトへの状態変更と同じ直列キュー）を通す。
// 間引きは `core-safety` の `thin_restore_points`（`update-ref -d` で `refs/hikae/` 配下の
// 復元点だけを削除）で行い、通常のブランチ・タグ・作業フォルダ・インデックスには触れない。

use std::collections::HashSet;

use core_ops::{ExcludeOutcome, Ops};
use core_safety::{thin_restore_points, ThinPolicy, BACKUP_RETENTION_DAYS};
use core_store::{
    journal_cutoff_from_now, now_rfc3339, AppSettings, JournalOutcome, JournalTrigger,
    NewJournalEntry, Project,
};
use tauri_specta::Event;
use time::OffsetDateTime;

use crate::events::{AttentionReason, NeedsAttention, StatusChanged};
use crate::health::HealthChange;
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
    let signing = ctx.signing_user();
    let health = ctx.health.clone();
    let project_for_health = project.clone();

    let outcome = run_exclusive(
        ctx.locks.clone(),
        project.id.clone(),
        "ファイルは安全です",
        move || {
            let mut deleted = 0usize;
            let mut health_change = HealthChange::default();

            // 順番を待つあいだに「一覧から外す」が実行されていたら、何もしない
            let registered = store
                .lock()
                .ok()
                .is_some_and(|g| g.get_project(&id).is_ok());
            if !registered {
                return Ok((0, HealthChange::default()));
            }

            // リポジトリの破損（E10）と保存の容量（E09）を軽く確かめる（読み取りのみ。何も削除しない）。
            // 壊れていると分かったプロジェクトには、以降の書き込みをしない
            if path.is_dir() {
                let ops = Ops::new(crate::git_runner());
                health_change = health.refresh(&ops, &project_for_health);
                if health.is_broken(&id) {
                    return Ok((0, health_change));
                }
            }

            // 一時ファイルの除外設定（管理する部分がまだ無いプロジェクトに 1 回だけ）
            if path.is_dir() {
                let ops = Ops::new(crate::git_runner())
                    .with_signing_user(signing.as_ref().map(|(uid, login)| (*uid, login.as_str())));
                let started_at = now_rfc3339();
                if let Ok(ExcludeOutcome::Written) = ops.apply_default_excludes(&path, true) {
                    if let Ok(guard) = store.lock() {
                        let _ = guard.record_journal(&NewJournalEntry {
                            project_id: id.clone(),
                            operation: "set-excludes".to_string(),
                            trigger: JournalTrigger::Auto,
                            started_at,
                            finished_at: now_rfc3339(),
                            outcome: JournalOutcome::Success,
                            detail: Some("applied".to_string()),
                            snapshot_ref: None,
                            backup_ref: None,
                            target: None,
                        });
                    }
                }
            }

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
            Ok((deleted, health_change))
        },
    )
    .await;

    let Ok((deleted, change)) = outcome else {
        return;
    };
    // 新しく見つかった問題は、画面に知らせる（状態の取得にも載るため、画面が開く前でも見落とさない）
    if change.became_broken {
        let _ = NeedsAttention {
            project_id: project.id.clone(),
            reason: AttentionReason::RepoBroken,
        }
        .emit(&ctx.app);
    }
    if change.became_large {
        let _ = NeedsAttention {
            project_id: project.id.clone(),
            reason: AttentionReason::RepoLarge,
        }
        .emit(&ctx.app);
    }
    // 履歴の自動保存の一覧が変わる、または健全性の表示が変わるため、画面に再取得させる
    if deleted > 0 || change.changed {
        let _ = StatusChanged {
            project_id: project.id.clone(),
        }
        .emit(&ctx.app);
    }
}
