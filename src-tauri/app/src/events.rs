// Rust から UI へ通知するイベント（設計書 9.3）。
// tauri-specta で型付けし、`src/lib/bindings.ts` に `events` として出力される。

use serde::{Deserialize, Serialize};

/// 操作の起動元
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub enum OpTrigger {
    /// ユーザーの操作
    #[serde(rename = "manual")]
    Manual,
    /// スケジューラによる自動実行
    #[serde(rename = "auto")]
    Auto,
}

/// 同期状態（オフライン・認証失敗は例外ではなく状態として扱う）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub enum SyncStateKind {
    #[serde(rename = "idle")]
    Idle,
    /// オフライン。自動で再試行する
    #[serde(rename = "offline")]
    Offline,
    /// 再認証が必要
    #[serde(rename = "auth-required")]
    AuthRequired,
    /// 変更のぶつかりの解消待ち
    #[serde(rename = "conflicted")]
    Conflicted,
    /// そのほかの失敗。自動で再試行する
    #[serde(rename = "error")]
    Error,
}

/// ユーザーの対応が必要な理由
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub enum AttentionReason {
    /// 変更のぶつかりがある
    #[serde(rename = "conflict")]
    Conflict,
    /// 再認証が必要
    #[serde(rename = "auth")]
    Auth,
    /// 未保存の変更があるため、自動の取り込みを見送った
    #[serde(rename = "unsaved-changes")]
    UnsavedChanges,
    /// 取り込み前の自動保存に大きいファイルがあるため、自動の取り込みを見送った（E07 / E08）
    #[serde(rename = "large-files")]
    LargeFiles,
    /// 前回のアプリ終了で、途中で止まった操作が見つかった（E15）
    #[serde(rename = "interrupted-operation")]
    InterruptedOperation,
    /// 登録したフォルダが見つからない・フォルダでない・リポジトリでない（E11）。自動の取り込み・
    /// アップロードは、フォルダが戻るまで見送る
    #[serde(rename = "folder-missing")]
    FolderMissing,
    /// 保存のデータが 1GB を超えた（E09）。自動では何も削除しない
    #[serde(rename = "repo-large")]
    RepoLarge,
    /// リポジトリの記録が読めなくなっている疑いがある（E10）。自動の取り込み・アップロードは見送る
    #[serde(rename = "repo-broken")]
    RepoBroken,
    /// 取り込む側に、この PC では作れないファイル名があるため、自動の取り込みを見送った（E18）
    #[serde(rename = "unsupported-file-names")]
    UnsupportedFileNames,
}

/// プロジェクトの状態が変わった。UI は `[projectId]` 配下のキャッシュを無効化して再取得する。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StatusChanged {
    pub project_id: String,
}

/// プロジェクトのフォルダでファイルが変わった（ファイル監視。保存の対象になりうる変更だけ）。
/// UI は変更一覧のキャッシュだけを無効化して取り直す（重い再取得を避けるため `status_changed` とは分ける）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct FilesChanged {
    pub project_id: String,
}

/// 状態変更操作を開始した
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct OpProgress {
    pub project_id: String,
    /// `save` / `restore` / `pull` / `push` / `resolve`
    pub operation: String,
    pub trigger: OpTrigger,
}

/// 状態変更操作が終わった
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct OpFinished {
    pub project_id: String,
    pub operation: String,
    pub trigger: OpTrigger,
    pub ok: bool,
    /// 結果の要約（`merged` / `up-to-date` / 失敗の種類など。エラー本文は含まない）
    pub outcome: String,
}

/// 同期状態が変わった
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct SyncStateChanged {
    pub project_id: String,
    pub state: SyncStateKind,
    /// 次に自動で再試行する時刻（Unix 秒）。再試行待ちでなければ null
    pub retry_at: Option<f64>,
}

/// ユーザーの対応が必要になった
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct NeedsAttention {
    pub project_id: String,
    pub reason: AttentionReason,
}

/// GitHub から取得する処理の段階
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub enum ClonePhase {
    /// 取得先や認証の確認中
    #[serde(rename = "preparing")]
    Preparing,
    /// ファイルをダウンロード中（git の実行中。割合は取得できない）
    #[serde(rename = "downloading")]
    Downloading,
    /// ダウンロード後の仕上げ（登録）中
    #[serde(rename = "finishing")]
    Finishing,
    /// 完了した
    #[serde(rename = "done")]
    Done,
    /// 失敗した（エラーは呼び出しの戻り値で返す）
    #[serde(rename = "failed")]
    Failed,
}

/// GitHub からの取得の進行状況。実行中のプロジェクトは一覧にまだ無いため、呼び出し時に渡した ID で識別する。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct CloneProgress {
    pub project_id: String,
    pub phase: ClonePhase,
}
