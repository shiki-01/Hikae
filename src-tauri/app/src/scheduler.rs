// 起動時・定期の取り込みと、保存後のアップロードを行うスケジューラ（設計書 4.3 / 4.5 / 7章）。
//
// 実行計画の判断（次回実行時刻・バックオフ）は tauri 非依存の `core_watch::SyncPlanner` が行う。
// ここは計画に従って実行し、結果を計画へ戻し、状態の変化を UI へ通知するだけの薄い層。
// 実行は必ず `run_op`（= `run_exclusive`）を通すため、同一プロジェクトへの状態変更は直列に実行される。

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use core_ops::{PullOutcome, UploadOutcome};
use core_store::{AppSettings, Project};
use core_watch::{FailureKind, SyncHealth, SyncPlanner, SyncPolicy, SyncTask, TaskResult};
use tauri::Manager;
use tauri_specta::Event;

use crate::events::{AttentionReason, NeedsAttention, OpTrigger, SyncStateChanged, SyncStateKind};
use crate::ops_runner::{
    failure_kind, run_op, summarize_pull, summarize_push, OpContext, OpFailure, OpSpec,
};
use crate::AppState;

/// 待機の上限（秒）。設定の変更やプロジェクトの追加を拾うために定期的に起きる。
const MAX_SLEEP_SECS: u64 = 30;

struct Inner {
    started: Instant,
    planners: Mutex<HashMap<String, SyncPlanner>>,
    wake_tx: Sender<()>,
    wake_rx: Mutex<Option<Receiver<()>>>,
}

/// スケジューラへの窓口。コマンドから「保存した」「手動で取り込んだ」を伝えるために使う。
#[derive(Clone)]
pub struct SchedulerHandle {
    inner: Arc<Inner>,
}

impl SchedulerHandle {
    pub fn new() -> Self {
        let (wake_tx, wake_rx) = channel();
        SchedulerHandle {
            inner: Arc::new(Inner {
                started: Instant::now(),
                planners: Mutex::new(HashMap::new()),
                wake_tx,
                wake_rx: Mutex::new(Some(wake_rx)),
            }),
        }
    }

    /// 起動からの経過秒（計画の時刻軸。単調増加）
    fn now(&self) -> u64 {
        self.inner.started.elapsed().as_secs()
    }

    /// 計画の時刻を Unix 秒へ換算する（UI への通知用）
    fn unix_at(&self, at: u64) -> f64 {
        let unix_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now = self.now();
        (unix_now + at.saturating_sub(now)) as f64
    }

    fn wake(&self) {
        let _ = self.inner.wake_tx.send(());
    }

    /// 登録や設定が変わったので、直ちに計画を作り直させる。
    pub fn refresh(&self) {
        self.wake();
    }

    fn with_planner(&self, id: &str, f: impl FnOnce(&mut SyncPlanner, u64)) {
        let now = self.now();
        if let Ok(mut map) = self.inner.planners.lock() {
            // まだ計画のないプロジェクト（追加直後）は、次の巡回で起動時の取り込みから始まる
            if let Some(planner) = map.get_mut(id) {
                f(planner, now);
            }
        }
        self.wake();
    }

    /// 保存が完了した。「保存時に自動アップロード」がオンならアップロードが予定される。
    pub fn request_push(&self, id: &str) {
        self.with_planner(id, |p, now| p.request_push(now));
    }

    /// ぶつかりの解消後など、取り込みを直ちに予定する。
    pub fn request_pull(&self, id: &str) {
        self.with_planner(id, |p, now| p.request_pull(now));
    }

    /// 手動で行った取り込み・アップロードの結果を計画へ反映する
    /// （成功すれば失敗状態が解除され、失敗すれば再試行の間隔に従う）。
    pub fn note_result(&self, id: &str, task: SyncTask, result: TaskResult) {
        self.with_planner(id, |p, now| p.report(task, result, now));
    }

    fn take_receiver(&self) -> Option<Receiver<()>> {
        self.inner.wake_rx.lock().ok().and_then(|mut r| r.take())
    }

    /// 設定と登録内容を計画へ反映する。リモートのないプロジェクトは対象外。
    fn sync_planners(&self, targets: &[(Project, SyncPolicy)]) {
        let now = self.now();
        let Ok(mut map) = self.inner.planners.lock() else {
            return;
        };
        let wanted: HashSet<&str> = targets.iter().map(|(p, _)| p.id.as_str()).collect();
        map.retain(|id, _| wanted.contains(id.as_str()));
        for (project, policy) in targets {
            match map.get_mut(&project.id) {
                Some(planner) => planner.set_policy(*policy, now),
                None => {
                    map.insert(project.id.clone(), SyncPlanner::new(*policy, now));
                }
            }
        }
    }

    fn next_task(&self, id: &str) -> Option<SyncTask> {
        let now = self.now();
        self.inner
            .planners
            .lock()
            .ok()
            .and_then(|map| map.get(id).and_then(|p| p.next_task(now)))
    }

    fn report(&self, id: &str, task: SyncTask, result: TaskResult) {
        let now = self.now();
        if let Ok(mut map) = self.inner.planners.lock() {
            if let Some(planner) = map.get_mut(id) {
                planner.report(task, result, now);
            }
        }
    }

    /// 各プロジェクトの同期状態と再試行予定（Unix 秒）
    fn states(&self) -> Vec<(String, SyncHealth, Option<f64>)> {
        let Ok(map) = self.inner.planners.lock() else {
            return Vec::new();
        };
        map.iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    p.health(),
                    p.retry_at().map(|t| self.unix_at(t)),
                )
            })
            .collect()
    }

    /// 次に起きるまでの待ち時間
    fn sleep_for(&self) -> Duration {
        let now = self.now();
        let next = self
            .inner
            .planners
            .lock()
            .ok()
            .and_then(|map| map.values().filter_map(|p| p.next_wake()).min());
        let secs = match next {
            Some(at) => at.saturating_sub(now).clamp(1, MAX_SLEEP_SECS),
            None => MAX_SLEEP_SECS,
        };
        Duration::from_secs(secs)
    }
}

impl Default for SchedulerHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// 設定から同期の方針を作る（起動時の取り込み・定期取り込みの間隔・保存時の自動アップロード）。
fn policy_from_settings(s: &AppSettings) -> SyncPolicy {
    SyncPolicy::from_config(
        Some(s.pull_on_startup),
        Some(s.pull_interval_minutes),
        Some(s.auto_push_after_save),
    )
}

fn state_kind(h: SyncHealth) -> SyncStateKind {
    match h {
        SyncHealth::Idle => SyncStateKind::Idle,
        SyncHealth::Offline => SyncStateKind::Offline,
        SyncHealth::AuthRequired => SyncStateKind::AuthRequired,
        SyncHealth::Conflicted => SyncStateKind::Conflicted,
        SyncHealth::Error => SyncStateKind::Error,
    }
}

/// 取り込みの結果を計画へ渡す形に変換する
pub(crate) fn pull_task_result(r: &Result<PullOutcome, OpFailure>) -> TaskResult {
    match r {
        Ok(PullOutcome::Conflicted { .. }) => TaskResult::Failed(FailureKind::Conflict),
        Ok(_) => TaskResult::Ok,
        Err(OpFailure::Ops(e)) => TaskResult::Failed(failure_kind(e)),
        Err(OpFailure::App(_)) => TaskResult::Failed(FailureKind::Other),
    }
}

/// アップロードの結果を計画へ渡す形に変換する
pub(crate) fn push_task_result(r: &Result<UploadOutcome, OpFailure>) -> TaskResult {
    match r {
        Ok(UploadOutcome::NeedsResolve(_)) => TaskResult::Failed(FailureKind::Conflict),
        Ok(_) => TaskResult::Ok,
        Err(OpFailure::Ops(e)) => TaskResult::Failed(failure_kind(e)),
        Err(OpFailure::App(_)) => TaskResult::Failed(FailureKind::Other),
    }
}

/// スケジューラを起動する（アプリの setup から 1 回だけ呼ぶ）
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(run_loop(app));
}

async fn run_loop(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    let handle = state.scheduler.clone();
    let ctx = OpContext::new(app.clone(), &state);
    let Some(mut rx) = handle.take_receiver() else {
        return;
    };
    // 通知済みの状態（変化したときだけ通知する）
    let mut notified: HashMap<String, (SyncHealth, Option<u64>)> = HashMap::new();

    loop {
        // 1) 登録内容と設定を計画へ反映
        // 設定（全体 + プロジェクト別の上書き。設計書 7章）を毎回読み直す
        let store = ctx.store.clone();
        let listed: Vec<(Project, AppSettings)> = tauri::async_runtime::spawn_blocking(move || {
            let guard = store.lock().ok()?;
            let projects = guard.list_projects().ok()?;
            Some(
                projects
                    .into_iter()
                    .map(|p| {
                        // 設定を読めない場合は 7章の既定値で動かす
                        let settings = guard.effective_settings(&p.id).unwrap_or_default();
                        (p, settings)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

        let targets: Vec<(Project, SyncPolicy, bool)> = listed
            .into_iter()
            .filter(|(p, _)| p.remote_url.as_deref().is_some_and(|u| !u.is_empty()))
            .map(|(p, s)| (p, policy_from_settings(&s), !s.save_before_pull))
            .collect();
        handle.sync_planners(
            &targets
                .iter()
                .map(|(p, policy, _)| (p.clone(), *policy))
                .collect::<Vec<_>>(),
        );

        // 2) 期限の来た処理を、プロジェクトごとに 1 件ずつ実行（取り込み → アップロードの順）
        for (project, _, skip_when_unsaved) in &targets {
            for _ in 0..2 {
                let Some(task) = handle.next_task(&project.id) else {
                    break;
                };
                let result = execute(&ctx, project, task, *skip_when_unsaved).await;
                handle.report(&project.id, task, result);
            }
        }

        // 3) 状態の変化を通知
        for (id, health, retry_at) in handle.states() {
            let key = (health, retry_at.map(|t| t as u64));
            let previous = notified
                .get(&id)
                .map(|(h, _)| *h)
                .unwrap_or(SyncHealth::Idle);
            let changed = notified
                .get(&id)
                .map(|prev| *prev != key)
                .unwrap_or(health != SyncHealth::Idle);
            if !changed {
                continue;
            }
            notified.insert(id.clone(), key);
            let _ = SyncStateChanged {
                project_id: id.clone(),
                state: state_kind(health),
                retry_at,
            }
            .emit(&app);
            if previous != health {
                let reason = match health {
                    SyncHealth::Conflicted => Some(AttentionReason::Conflict),
                    SyncHealth::AuthRequired => Some(AttentionReason::Auth),
                    _ => None,
                };
                if let Some(reason) = reason {
                    let _ = NeedsAttention {
                        project_id: id,
                        reason,
                    }
                    .emit(&app);
                }
            }
        }

        // 4) 次の予定まで待つ（コマンドからの通知で早く起きる）
        let timeout = handle.sleep_for();
        match tauri::async_runtime::spawn_blocking(move || {
            let _ = rx.recv_timeout(timeout);
            // 溜まった通知は 1 回の巡回でまとめて処理する
            while rx.try_recv().is_ok() {}
            rx
        })
        .await
        {
            Ok(returned) => rx = returned,
            Err(_) => break,
        }
    }
}

/// 1 件の処理を実行して結果を返す。オフライン・認証失敗は `TaskResult::Failed` として返し、例外にしない。
async fn execute(
    ctx: &OpContext,
    project: &Project,
    task: SyncTask,
    skip_when_unsaved: bool,
) -> TaskResult {
    match task {
        SyncTask::Pull => {
            if skip_when_unsaved && has_unsaved_changes(project).await {
                // 「取り込む前に未保存の変更を保存」が「確認する」のとき、自動では取り込まない
                let _ = NeedsAttention {
                    project_id: project.id.clone(),
                    reason: AttentionReason::UnsavedChanges,
                }
                .emit(&ctx.app);
                return TaskResult::Ok;
            }
            let r = run_op(
                ctx,
                &project.id,
                OpSpec {
                    operation: "pull",
                    trigger: OpTrigger::Auto,
                    target: None,
                    data_is_safe: "ファイルは安全です",
                },
                |ops, path| ops.pull(path),
                summarize_pull,
            )
            .await;
            pull_task_result(&r)
        }
        SyncTask::Push => {
            let r = run_op(
                ctx,
                &project.id,
                OpSpec {
                    operation: "push",
                    trigger: OpTrigger::Auto,
                    target: None,
                    data_is_safe: "ファイルは安全です",
                },
                |ops, path| ops.upload(path),
                summarize_push,
            )
            .await;
            push_task_result(&r)
        }
    }
}

/// 未保存の変更があるか（読み取りのみ。キューは通さない）
async fn has_unsaved_changes(project: &Project) -> bool {
    let path = project.path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ops = core_ops::Ops::new(crate::git_runner());
        ops.sync_state(&path).map(|s| s.dirty).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}
