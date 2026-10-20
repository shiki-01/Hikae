// 自動保存のクラウド控え（設計書 6.4）の app 層。
//
// - 送るのは、アップロード（手動・スケジューラ）の本体と同じ直列キューの内側。`upload_and_back_up` が
//   アップロードのあとに続けて `backup_snapshots_to_cloud` を呼ぶ（別の操作として記録しない）
// - 送る条件: 設定 `snapshot_cloud_backup` がオン、ログイン済み（署名に使うユーザーがいる）、保存先（origin）がある
// - 失敗（オフライン・認証など）は記録に残さず、何もしない。次のアップロードのときに再試行する
// - 控えた件数は、この起動で最後に成功した送信の `total_refs` だけをメモリに持つ（簡易版。再起動で「未確認」に戻る）
// - トークンは扱わない。認証は `crate::git_runner()` の credential helper が行う（不変条件 9）

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use core_ops::{Ops, OpsError, UploadOutcome};
use serde::{Deserialize, Serialize};

use crate::ops_runner::OpContext;
use crate::{git_runner, load_project, run_blocking, AppError, AppState};

/// プロジェクト ID ごとの、この起動で最後に成功した控えの総数
pub(crate) type CloudCounts = Arc<Mutex<HashMap<String, u32>>>;

/// アップロードのあとに控えるための情報（プロジェクト ID と、件数を記録する場所）。控えないときは None
pub(crate) type Plan = Option<(CloudCounts, String)>;

/// 設定がオンで、ログイン済みなら控えの計画を作る（キューの外で読む。読み取りのみ）。
/// 保存先の有無は、キューの内側で `origin_url` を見て確かめる。
pub(crate) fn plan_for(ctx: &OpContext, project_id: &str) -> Plan {
    let enabled = ctx
        .store
        .lock()
        .ok()
        .and_then(|g| g.get_settings().ok())
        .is_some_and(|s| s.snapshot_cloud_backup);
    if enabled && ctx.signing_user().is_some() {
        Some((ctx.cloud_counts.clone(), project_id.to_string()))
    } else {
        None
    }
}

/// アップロードの本体（キューの内側で呼ぶ）。アップロードが成功したあと、計画があれば自動保存を控える。
/// 控えの失敗はアップロードの結果を変えず、記録にも残さない（次の機会に再試行する）。
pub(crate) fn upload_and_back_up(
    ops: &Ops,
    path: &Path,
    limits: core_ops::SizeLimits,
    plan: Plan,
) -> Result<UploadOutcome, OpsError> {
    let outcome = ops.upload_with(path, limits)?;
    if let Some((counts, project_id)) = plan {
        back_up_snapshots(ops, path, &counts, &project_id);
    }
    Ok(outcome)
}

/// 保存先があるときだけ、自動保存を控える（キューの内側）。結果は件数の記録にだけ使う。
fn back_up_snapshots(ops: &Ops, path: &Path, counts: &CloudCounts, project_id: &str) {
    if !matches!(ops.origin_url(path), Ok(Some(url)) if !url.is_empty()) {
        return;
    }
    if let Ok(outcome) = ops.backup_snapshots_to_cloud(path) {
        if let Ok(mut g) = counts.lock() {
            g.insert(project_id.to_string(), outcome.total_refs);
        }
    }
}

/// 画面の「自動保存」タブに出す、クラウド控えの状態
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CloudBackupStatus {
    /// 設定「自動保存をクラウドにも控える」がオンか
    pub enabled: bool,
    /// ローカルにある自動保存の数
    pub local_count: u32,
    /// この起動で最後に控えた総数。まだ控えていない・再起動後は null（画面は「未確認」と出す）
    pub cloud_count: Option<u32>,
}

/// クラウド控えの状態を返す（読み取りのみ。キューは通さない）
#[tauri::command]
#[specta::specta]
pub(crate) async fn cloud_backup_status(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<CloudBackupStatus, AppError> {
    let store = state.store_clone();
    let counts = state.cloud_counts.clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        let enabled = store
            .lock()
            .ok()
            .and_then(|g| g.get_settings().ok())
            .is_some_and(|s| s.snapshot_cloud_backup);
        let local_count = Ops::new(git_runner())
            .local_snapshot_count(&project.path)
            .map_err(AppError::from_ops_error)?;
        let cloud_count = counts.lock().ok().and_then(|g| g.get(&id).copied());
        Ok(CloudBackupStatus {
            enabled,
            local_count,
            cloud_count,
        })
    })
    .await
}

/// `CloudCounts` の初期値（起動時は空。まだ控えていない）
pub(crate) fn new_counts() -> CloudCounts {
    Arc::new(Mutex::new(HashMap::new()))
}
