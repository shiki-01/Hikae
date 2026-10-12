// ローカル LLM（設計書 10 章）の app 層。
//
// - モデルのカタログ、導入状態、導入できる理由（メモリ・空き容量）、ダウンロード・中止・削除
// - 保存メモの提案（`ai_suggest_memo`・`ai_cancel_suggest`）、接続先の確認、生成の試行（設定の「試す」）
// - 提案はボタンで開始する。結果が得られない理由（未導入・Ollama 未接続・時間切れ・検証不合格・取り消し）は
//   `SuggestFailure` で返し、画面がその理由に応じた文言を出す
// - 提案の上限は `GENERATE_TIMEOUT`（90 秒）の 1 つ。入力の収集の時間を差し引いた残りを生成に渡す。内蔵の起動待ちは別枠
// - 同時に走らせる提案は 1 件（`core_llm::SuggestSlot`）。新しい要求は進行中の要求を取り消す。切断しても llama-server は
//   生成を続け、次の要求が待たされる（実測で最大 17 秒）ため、取り消し・時間切れのときは llama-server を止める
//   （次の要求で起動し直す。新しい要求が始まっていれば止めない）
// - 提案は読み取りだけ。作業フォルダ・インデックス・refs には書かない。モデルの保存先はアプリのデータフォルダだけ
// - 設定の「試す」（`ai_test_generation`）だけ、失敗の理由を画面に出す（E19）
// - ダウンロードと削除は、モデル ID ごとに同時に 1 つだけ（`ModelSlots`）
// - ログ・エラーに、パス・ファイルの内容・トークンを出さない

use crate::events::{LlmDownloadFinished, LlmDownloadOutcome, LlmDownloadProgress};
use crate::office::{self, ParagraphRow};
use crate::{git_runner, load_project, run_blocking, AppError, AppState};
use core_llm::{
    cache_key, cell_amount, cell_line, check_endpoint, installed, list_ollama_models,
    memo_from_text, memo_sensitive_kinds, memo_text, paragraph_amount, prepare_input,
    sensitive_file, signed_line, stage_availability, text_amount, validate_custom_gguf, Backend,
    CatalogEntry, ChangeInput, ChangeKind, CustomGgufError, Downloader, Installed, LlmError, Memo,
    ModelChoice, Prepared, Reason, SensitiveKind, ServiceConfig, SourceChange, SuggestFailure,
    SuggestNotice, SystemInfo, GENERATE_TIMEOUT,
};
use core_ops::{ChangedKind, MemoTextDiff, Ops, MEMO_TEXT_MAX_BYTES};
use core_preview::{office_kind, ExtensionHost, OfficeKind};
use core_store::{
    now_rfc3339, AiModelSource, AiRunner, AiScope, AppSettings, LlmModelRecord, MemoSuggestion,
    SettingsPatch, Store,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;
use tokio_util::sync::CancellationToken;

/// 提案に渡す変更ファイルの上限（これより後ろは渡さない）
const MAX_INPUT_FILES: usize = 100;
/// 進捗イベントを出す間隔（バイト）。1 MiB ごと、および完了時
const PROGRESS_STEP_BYTES: u64 = 1024 * 1024;
/// 内蔵の llama-server の実行ファイル名
#[cfg(windows)]
const SERVER_EXE: &str = "llama-server.exe";
#[cfg(not(windows))]
const SERVER_EXE: &str = "llama-server";

// ========== 画面に返す型 ==========

/// PC の状態（導入の可否の判定に使う値）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub struct SystemView {
    /// 総メモリ（バイト）
    pub total_memory_bytes: f64,
    /// モデルを置くドライブの空き容量（バイト）。判定できないときは 0
    pub free_disk_bytes: f64,
}

/// 導入状態
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ModelStatus {
    Missing,
    Partial,
    Ready,
}

/// 導入できない理由（PC の条件）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UnavailableReason {
    /// メモリが足りない（必要量と PC の総メモリ）
    NotEnoughMemory { needed: f64, have: f64 },
    /// 空き容量が足りない（必要量と空き容量）
    NotEnoughDisk { needed: f64, have: f64 },
}

/// カタログの 1 モデルと、この PC での状態
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelView {
    pub id: String,
    pub display_name: String,
    pub stage: u8,
    pub size_bytes: f64,
    pub license_name: String,
    pub license_url: String,
    /// 必要メモリの目安（バイト）
    pub min_memory_bytes: f64,
    pub recommended: bool,
    pub status: ModelStatus,
    /// 途中まで取得したバイト数（`partial` のときだけ意味を持つ）
    pub partial_bytes: f64,
    /// この PC で導入できるか
    pub available: bool,
    /// 導入できないときの理由
    pub unavailable_reason: Option<UnavailableReason>,
    /// ダウンロード中か
    pub downloading: bool,
}

/// AI タブの一覧
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AiCatalog {
    pub system: SystemView,
    pub models: Vec<ModelView>,
}

/// 接続先の確認の結果
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub struct EndpointCheck {
    /// 接続先が使える（形式が正しく、Ollama が応答した）
    pub ok: bool,
    /// ループバック以外のホストか（画面で警告する）
    pub external_host: bool,
}

/// 生成の試行が失敗した理由（設定の「試す」の結果。文言は画面が選ぶ）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum TestFailure {
    /// モデルが選ばれていない、または接続先の指定が正しくない
    NotConfigured,
    /// 内蔵のモデルが導入されていない
    NotInstalled,
    /// llama-server が見つからない
    ServerNotFound,
    /// llama-server の起動に失敗した
    ServerStartFailed,
    /// llama-server の起動が時間内に終わらなかった
    ServerStartTimeout,
    /// 生成が時間内に終わらなかった
    Timeout,
    /// 接続先に接続できなかった
    Network,
    /// 接続先が正常な応答を返さなかった
    BadStatus,
    /// 応答の形式が正しくなかった
    BadResponse,
    /// 接続先の URL が正しくない
    InvalidEndpoint,
    /// 自分で用意した GGUF ファイルが使えない（存在・形式・サイズの検査で拒否された）
    CustomInvalid,
    /// 生成されたが検証で弾かれた
    Rejected,
    /// そのほか
    Other,
}

/// カスタム GGUF の検査が拒否した理由（画面が文言を選ぶ。パス・ファイルの内容は返さない）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum CustomGgufReason {
    /// パスが空
    Empty,
    /// パスに使えない文字がある
    ControlChar,
    /// ファイルが見つからない
    NotFound,
    /// リンクは使えない
    Link,
    /// ファイルではない（フォルダなど）
    NotAFile,
    /// 拡張子が .gguf ではない
    WrongExtension,
    /// 12GB を超える
    TooLarge,
    /// GGUF の形式ではない
    NotGguf,
    /// 読めない（権限など）
    Unreadable,
}

/// カスタム GGUF の検査の結果（読み取りのみ）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub struct CustomGgufCheck {
    /// 使える
    pub ok: bool,
    /// 使えないときの理由
    pub reason: Option<CustomGgufReason>,
}

/// 生成の試行の結果
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub struct GenerationTest {
    pub ok: bool,
    /// 失敗のときの理由
    pub reason: Option<TestFailure>,
}

/// 保存メモの提案の結果。`title` があれば成功（`body` は本文の行を改行でつないだもの。無ければ null）。
/// 無ければ `failure` の理由。`notices` は、入力の伏せ字・機微なファイルの除外・本文の省略を画面に短く知らせる
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SuggestResult {
    pub title: Option<String>,
    pub body: Option<String>,
    pub failure: Option<SuggestFailure>,
    pub notices: Vec<SuggestNotice>,
}

impl SuggestResult {
    fn ok(suggested: Suggested) -> Self {
        let Suggested { memo, notices } = suggested;
        Self {
            title: Some(memo.title),
            body: (!memo.body.is_empty()).then(|| memo.body.join("\n")),
            failure: None,
            notices,
        }
    }

    fn failed(failure: SuggestFailure) -> Self {
        Self {
            title: None,
            body: None,
            failure: Some(failure),
            notices: Vec::new(),
        }
    }
}

/// 生成に成功した提案（検証を通ったメモと、入力の注記）
struct Suggested {
    memo: Memo,
    notices: Vec<SuggestNotice>,
}

// ========== ダウンロード・削除の直列化 ==========

/// モデルの操作の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Download,
    Remove,
}

/// モデル ID ごとに、進行中のダウンロードまたは削除を 1 つだけ許す。
/// 取得・解放は `SlotGuard` が行うため、途中で失敗しても解放されない状態は残らない。
#[derive(Default)]
pub struct ModelSlots {
    inner: Mutex<HashMap<String, (SlotKind, CancellationToken)>>,
}

/// 取得した操作枠。破棄すると枠を解放する
pub struct SlotGuard {
    slots: Arc<ModelSlots>,
    id: String,
    token: CancellationToken,
}

impl SlotGuard {
    fn token(&self) -> CancellationToken {
        self.token.clone()
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.slots.inner.lock() {
            map.remove(&self.id);
        }
    }
}

impl ModelSlots {
    /// 枠を取る。同じ ID の操作が進行中なら `None`
    fn acquire(self: &Arc<Self>, id: &str, kind: SlotKind) -> Option<SlotGuard> {
        let mut map = self.inner.lock().ok()?;
        if map.contains_key(id) {
            return None;
        }
        let token = CancellationToken::new();
        map.insert(id.to_string(), (kind, token.clone()));
        Some(SlotGuard {
            slots: Arc::clone(self),
            id: id.to_string(),
            token,
        })
    }

    /// 進行中のダウンロードを中止する。ダウンロード以外（削除）や、無い ID は何もしない
    fn cancel_download(&self, id: &str) {
        if let Ok(map) = self.inner.lock() {
            if let Some((SlotKind::Download, token)) = map.get(id) {
                token.cancel();
            }
        }
    }

    /// 進行中のダウンロードがあるか
    fn is_downloading(&self, id: &str) -> bool {
        self.inner
            .lock()
            .map(|map| matches!(map.get(id), Some((SlotKind::Download, _))))
            .unwrap_or(false)
    }
}

// ========== 共通の部品 ==========

/// 内蔵の llama-server のパス（リソースフォルダの `llama/`）
pub(crate) fn bundled_server_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join("llama").join(SERVER_EXE)
}

/// 起動時に作る内蔵サービスの設定（保存先・同梱パス・接続先）
pub(crate) fn service_config(
    models_dir: PathBuf,
    resource_dir: Option<&Path>,
    settings: &AppSettings,
) -> ServiceConfig {
    ServiceConfig {
        models_dir,
        bundled_exe: resource_dir.map(bundled_server_path),
        backend: backend_of(settings),
    }
}

/// 設定から接続先を決める。内蔵か、Ollama（詳細設定の URL）
fn backend_of(settings: &AppSettings) -> Backend {
    match settings.ai_runner {
        AiRunner::Builtin => Backend::Builtin,
        AiRunner::Ollama => Backend::Ollama {
            base_url: settings.ai_ollama_url.clone(),
        },
    }
}

/// サービスの接続先を設定に合わせる。違うときだけ切り替える（内蔵の起動中のものは止める）
pub(crate) async fn apply_backend(
    llm: &tokio::sync::RwLock<core_llm::LlmService>,
    settings: &AppSettings,
) -> Result<(), AppError> {
    let want = backend_of(settings);
    if llm.read().await.backend() == &want {
        return Ok(());
    }
    llm.write()
        .await
        .set_backend(want)
        .await
        .map_err(|e| llm_error("llm_backend_failed", e))
}

/// 設定から提案に使うモデルの指定を決める（判定は `core_llm::ModelChoice` が行う）。
/// 指定が空（未選択・パス未入力）なら `None`
fn model_choice(settings: &AppSettings) -> Option<ModelChoice> {
    ModelChoice::from_parts(
        settings.ai_runner == AiRunner::Ollama,
        settings.ai_model_source == AiModelSource::CustomGguf,
        &settings.ai_model,
        &settings.ai_custom_gguf_path,
    )
}

/// カスタム GGUF の検査エラーを画面の理由に変える
fn custom_reason(e: CustomGgufError) -> CustomGgufReason {
    match e {
        CustomGgufError::Empty => CustomGgufReason::Empty,
        CustomGgufError::ControlChar => CustomGgufReason::ControlChar,
        CustomGgufError::NotFound => CustomGgufReason::NotFound,
        CustomGgufError::Link => CustomGgufReason::Link,
        CustomGgufError::NotAFile => CustomGgufReason::NotAFile,
        CustomGgufError::WrongExtension => CustomGgufReason::WrongExtension,
        CustomGgufError::TooLarge => CustomGgufReason::TooLarge,
        CustomGgufError::NotGguf => CustomGgufReason::NotGguf,
        CustomGgufError::Unreadable => CustomGgufReason::Unreadable,
    }
}

/// 利用者が用意した GGUF ファイルを検査する（読み取りのみ）。
/// ファイルはコピー・移動・書き換えしない。SHA-256 は検証しない（画面に注意書きを出す）。
/// 結果にはパスを含めない。
#[tauri::command]
#[specta::specta]
pub async fn ai_check_custom_gguf(path: String) -> CustomGgufCheck {
    let checked =
        tauri::async_runtime::spawn_blocking(move || validate_custom_gguf(Path::new(&path))).await;
    match checked {
        Ok(Ok(_)) => CustomGgufCheck {
            ok: true,
            reason: None,
        },
        Ok(Err(e)) => CustomGgufCheck {
            ok: false,
            reason: Some(custom_reason(e)),
        },
        Err(_) => CustomGgufCheck {
            ok: false,
            reason: Some(CustomGgufReason::Unreadable),
        },
    }
}

fn store_settings(store: &Arc<Mutex<Store>>) -> Result<AppSettings, AppError> {
    let guard = store.lock().map_err(|_| database_error())?;
    guard.get_settings().map_err(|_| database_error())
}

fn database_error() -> AppError {
    AppError {
        code: "database_error".to_string(),
        params: Vec::new(),
        what_happened: "データベースアクセスに失敗しました".to_string(),
        data_is_safe: "ファイルは安全です".to_string(),
        next_action: "もう一度試してください".to_string(),
        technical_info: None,
    }
}

fn io_error() -> AppError {
    AppError {
        code: "ai_model_io_failed".to_string(),
        params: Vec::new(),
        what_happened: "AI モデルのファイルを操作できませんでした".to_string(),
        data_is_safe: "作業フォルダのファイルは変更されていません".to_string(),
        next_action: "空き容量と権限を確認して、もう一度試してください".to_string(),
        technical_info: None,
    }
}

fn model_busy_error() -> AppError {
    AppError {
        code: "ai_model_busy".to_string(),
        params: Vec::new(),
        what_happened: "このモデルは別の操作の途中です".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "終わってから、もう一度試してください".to_string(),
        technical_info: None,
    }
}

fn unknown_model_error() -> AppError {
    AppError {
        code: "ai_unknown_model".to_string(),
        params: Vec::new(),
        what_happened: "選ばれたモデルが一覧にありません".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "モデルの一覧から選び直してください".to_string(),
        technical_info: None,
    }
}

fn catalog_error() -> AppError {
    AppError {
        code: "llm_catalog_failed".to_string(),
        params: Vec::new(),
        what_happened: "モデルの一覧を読み込めませんでした".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "アプリを更新するか、もう一度起動してください".to_string(),
        technical_info: None,
    }
}

/// 内蔵の LLM の失敗を画面のエラーにする。文言は `LlmError` の固定文言だけを使う
fn llm_error(code: &str, e: LlmError) -> AppError {
    AppError {
        code: code.to_string(),
        params: Vec::new(),
        what_happened: e.to_string(),
        data_is_safe: "作業フォルダのファイルは変更されていません".to_string(),
        next_action: "設定を確認して、もう一度試してください".to_string(),
        technical_info: None,
    }
}

/// カタログの ID からモデルを取る
fn catalog_entry(id: &str) -> Result<CatalogEntry, AppError> {
    let catalog = core_llm::Catalog::builtin().map_err(|_| catalog_error())?;
    catalog.get(id).cloned().ok_or_else(unknown_model_error)
}

/// PC の総メモリと、モデルを置くドライブの空き容量
fn system_info(models_dir: &Path) -> SystemInfo {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total_memory_bytes = system.total_memory();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // モデルの保存先を含む、最も深いマウントポイントのドライブを選ぶ
    let free_disk_bytes = disks
        .list()
        .iter()
        .filter(|d| models_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0);
    SystemInfo {
        total_memory_bytes,
        free_disk_bytes,
    }
}

// ========== カタログと導入 ==========

/// カタログの一覧と、この PC での状態・可否を返す（読み取りのみ）。
#[tauri::command]
#[specta::specta]
pub async fn ai_catalog(state: State<'_, AppState>) -> Result<AiCatalog, AppError> {
    let models_dir = state.models_dir.clone();
    let slots = Arc::clone(&state.model_slots);
    run_blocking(move || {
        let catalog = core_llm::Catalog::builtin().map_err(|_| catalog_error())?;
        let info = system_info(&models_dir);
        let mut models = Vec::with_capacity(catalog.models.len());
        for entry in &catalog.models {
            let (status, partial_bytes) =
                match installed(&models_dir, entry).map_err(|_| io_error())? {
                    Installed::Missing => (ModelStatus::Missing, 0),
                    Installed::Partial { bytes } => (ModelStatus::Partial, bytes),
                    Installed::Ready => (ModelStatus::Ready, entry.size_bytes),
                };
            let availability = stage_availability(info, entry);
            let unavailable_reason = availability.reason.map(|reason| match reason {
                Reason::NotEnoughMemory { needed, have } => UnavailableReason::NotEnoughMemory {
                    needed: needed as f64,
                    have: have as f64,
                },
                Reason::NotEnoughDisk { needed, have } => UnavailableReason::NotEnoughDisk {
                    needed: needed as f64,
                    have: have as f64,
                },
            });
            models.push(ModelView {
                id: entry.id.clone(),
                display_name: entry.display_name.clone(),
                stage: entry.stage,
                size_bytes: entry.size_bytes as f64,
                license_name: entry.license_name.clone(),
                license_url: entry.license_url.clone(),
                min_memory_bytes: entry.min_total_memory_bytes as f64,
                recommended: entry.recommended,
                status,
                partial_bytes: partial_bytes as f64,
                available: availability.ok,
                unavailable_reason,
                downloading: slots.is_downloading(&entry.id),
            });
        }
        Ok(AiCatalog {
            system: SystemView {
                total_memory_bytes: info.total_memory_bytes as f64,
                free_disk_bytes: info.free_disk_bytes as f64,
            },
            models,
        })
    })
    .await
}

/// モデルのダウンロードを始める。バックグラウンドで実行し、すぐ戻る。
/// 進捗は `llm-download-progress`、完了・失敗・中止は `llm-download-finished` で知らせる。
#[tauri::command]
#[specta::specta]
pub async fn ai_download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let entry = catalog_entry(&id)?;
    let guard = Arc::clone(&state.model_slots)
        .acquire(&id, SlotKind::Download)
        .ok_or_else(model_busy_error)?;
    let token = guard.token();
    let models_dir = state.models_dir.clone();
    let store = state.store_clone();

    tauri::async_runtime::spawn(async move {
        let progress_app = app.clone();
        let progress_id = entry.id.clone();
        let mut last_reported = 0u64;
        let result = match Downloader::new(models_dir) {
            Ok(downloader) => {
                downloader
                    .download(&entry, &token, move |received, total| {
                        if received == total
                            || received.saturating_sub(last_reported) >= PROGRESS_STEP_BYTES
                        {
                            last_reported = received;
                            let _ = LlmDownloadProgress {
                                id: progress_id.clone(),
                                received: received as f64,
                                total: total as f64,
                            }
                            .emit(&progress_app);
                        }
                    })
                    .await
            }
            Err(e) => Err(e),
        };
        let outcome = match result {
            Ok(_) => {
                record_install(&store, &entry);
                LlmDownloadOutcome::Ready
            }
            Err(core_llm::DownloadError::Canceled) => LlmDownloadOutcome::Canceled,
            Err(_) => LlmDownloadOutcome::Failed,
        };
        // 枠を先に解放してから知らせる（完了を受けた画面がすぐ次の操作を始められるように）
        drop(guard);
        let _ = LlmDownloadFinished {
            id: entry.id.clone(),
            outcome,
        }
        .emit(&app);
    });
    Ok(())
}

/// 導入できたモデルを記録する（記録の失敗は導入の成否に影響させない。状態はファイルで判定する）
fn record_install(store: &Arc<Mutex<Store>>, entry: &CatalogEntry) {
    let Ok(guard) = store.lock() else {
        return;
    };
    let record = LlmModelRecord {
        id: entry.id.clone(),
        stage: entry.stage,
        file_path: format!("{}/{}", entry.id, entry.file_name),
        sha256: Some(entry.sha256.clone()),
        size_bytes: i64::try_from(entry.size_bytes).ok(),
        installed_at: now_rfc3339(),
    };
    let _ = guard.upsert_llm_model(&record);
}

/// 進行中のダウンロードを中止する。途中まで取得したファイルは残す（もう一度実行すると続きから）。
#[tauri::command]
#[specta::specta]
pub async fn ai_cancel_download(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.model_slots.cancel_download(&id);
    Ok(())
}

/// モデルを削除する。先に内蔵の llama-server を止める。ダウンロード中は拒否する。
/// 選ばれていたモデルを削除したときは、選択を空に戻す。
#[tauri::command]
#[specta::specta]
pub async fn ai_remove_model(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let entry = catalog_entry(&id)?;
    let guard = Arc::clone(&state.model_slots)
        .acquire(&id, SlotKind::Remove)
        .ok_or_else(model_busy_error)?;
    state.llm.read().await.stop().await;
    let models_dir = state.models_dir.clone();
    let store = state.store_clone();
    run_blocking(move || {
        // 枠は削除の終わりまで保持する
        let _guard = guard;
        core_llm::remove(&models_dir, &entry).map_err(|_| io_error())?;
        let store = store.lock().map_err(|_| database_error())?;
        store
            .remove_llm_model(&entry.id)
            .map_err(|_| database_error())?;
        let settings = store.get_settings().map_err(|_| database_error())?;
        if settings.ai_model == entry.id {
            store
                .update_settings(&SettingsPatch {
                    ai_model: Some(String::new()),
                    ..SettingsPatch::default()
                })
                .map_err(|_| database_error())?;
        }
        Ok(())
    })
    .await
}

// ========== 保存メモの提案 ==========

/// 保存メモの提案を 1 文作る（読み取りのみ）。ボタンを押したときだけ呼ばれる。
///
/// `fresh` が偽なら、同じ入力の保存済みの結果を返す。真なら保存済みの結果を使わずに作り直し、結果で上書きする。
/// 進行中の要求があれば取り消してから始める（同時に走らせる提案は 1 件）。
#[tauri::command]
#[specta::specta]
pub async fn ai_suggest_memo(app: AppHandle, project_id: String, fresh: bool) -> SuggestResult {
    // 戻り値を `SuggestResult` にするため、状態は引数で借りず、ハンドルから取り出す
    let state = app.state::<AppState>();
    let (seq, cancel) = state.suggest.begin();
    let outcome = match cancel
        .run_until_cancelled(suggest_for_project(&state, project_id, fresh))
        .await
    {
        Some(result) => result,
        None => Err(SuggestFailure::Cancelled),
    };
    let latest = state.suggest.finish(seq);
    // 取り消し・時間切れのとき、llama-server は切断しても生成を続け、次の要求が待たされる（実測で最大 17 秒）。
    // 止めれば次の要求は起動し直すだけ（3〜6 秒）で済む。新しい要求が始まっていれば、それが使うため止めない
    if latest
        && matches!(
            outcome,
            Err(SuggestFailure::Cancelled | SuggestFailure::Timeout)
        )
    {
        state.llm.read().await.stop().await;
    }
    match outcome {
        Ok(suggested) => SuggestResult::ok(suggested),
        Err(failure) => SuggestResult::failed(failure),
    }
}

/// 保存欄のメモに、パスワード・鍵・個人情報のように見える文字列があるかを調べる（読み取りのみ）。
/// 内容は返さず、種類だけを返す（設計書 10.6）。保存は止めない。警告と確認に使う。
#[tauri::command]
#[specta::specta]
pub fn check_memo_sensitive(text: String) -> Vec<SensitiveKind> {
    memo_sensitive_kinds(&text)
}

/// 進行中の保存メモの提案を取り消す。進行中の要求の側が、後始末として llama-server を止める
/// （`ai_suggest_memo`）。進行中が無ければ何もしない。
#[tauri::command]
#[specta::specta]
pub fn ai_cancel_suggest(state: State<'_, AppState>) {
    state.suggest.cancel_current();
}

async fn suggest_for_project(
    state: &AppState,
    project_id: String,
    fresh: bool,
) -> Result<Suggested, SuggestFailure> {
    let store = state.store_clone();
    let (settings, repo) = run_blocking(move || {
        // 有効・無効（memo_suggestion）はプロジェクト別に上書きできるため、そのプロジェクトの実効値を使う
        let settings = {
            let guard = store.lock().map_err(|_| database_error())?;
            guard
                .effective_settings(&project_id)
                .map_err(|_| database_error())?
        };
        let project = load_project(&store, &project_id)?;
        Ok((settings, project.path))
    })
    .await
    .map_err(|_| SuggestFailure::Unavailable)?;
    if settings.memo_suggestion != MemoSuggestion::Ai {
        return Err(SuggestFailure::NotReady);
    }
    let choice = model_choice(&settings).ok_or(SuggestFailure::NotReady)?;
    let model = choice.key();
    let ollama = settings.ai_runner == AiRunner::Ollama;
    // 本文も提案するか（既定はオン。オフのときは要約だけを求める。設計書 10.6）
    let body = settings.ai_suggest_body;
    apply_backend(&state.llm, &settings)
        .await
        .map_err(|_| SuggestFailure::NotReady)?;

    // 内蔵のモデルは導入済みのものだけ使う。カスタムは検査を通るものだけ使う（未導入・不備なら準備ができていない）
    let ready = match &choice {
        ModelChoice::Catalog(id) => {
            let llm = state.llm.read().await;
            llm.is_model_ready(id).unwrap_or(false)
        }
        ModelChoice::Custom(path) => {
            let path = path.clone();
            run_blocking(move || Ok(validate_custom_gguf(&path).is_ok()))
                .await
                .unwrap_or(false)
        }
        ModelChoice::Ollama(_) => true,
    };
    if !ready {
        return Err(SuggestFailure::NotReady);
    }

    // 接続先が外部ホストの Ollama のときは、ファイルの内容（差分の本文）を外へ送らない
    // （設計書 10.5 のプライバシー）。ファイル名と変更の種類だけを渡す
    let external_ollama =
        ollama && check_endpoint(&settings.ai_ollama_url).is_ok_and(|e| e.external_host);
    let scope = if external_ollama {
        AiScope::FileNames
    } else {
        settings.ai_scope
    };

    // 上限（90 秒）は入力の収集から始まる。内蔵の起動待ちは別枠
    let started = Instant::now();
    let extensions = state.extensions.clone();
    let Prepared { input, notices } = tokio::time::timeout(
        GENERATE_TIMEOUT,
        run_blocking(move || collect_input(&extensions, &repo, scope)),
    )
    .await
    .map_err(|_| SuggestFailure::Timeout)?
    .map_err(|_| SuggestFailure::Unavailable)?;
    if input.files.is_empty() {
        return Err(SuggestFailure::NotReady);
    }
    // キーは伏せ字の後の入力から作る（元の機微な文字列は残らない）。本文の有無でプロンプトが変わるため、キーも変わる
    let key = cache_key(&input, &model, body);

    if !fresh {
        let store = state.store_clone();
        let lookup_key = key.clone();
        // キャッシュの読み取りの失敗は、作り直しで扱う（提案の成否に影響させない）
        if let Ok(Some(hit)) = run_blocking(move || cached_message(&store, &lookup_key)).await {
            if let Some(memo) = memo_from_text(&hit) {
                // 提案の全文の最後の確認（設計書 10.6）。該当があれば、その提案は出さない
                if !memo_sensitive_kinds(&hit).is_empty() {
                    return Err(SuggestFailure::Invalid);
                }
                return Ok(Suggested { memo, notices });
            }
        }
    }

    // 生成の上限は、入力の収集の時間を差し引いた残り
    let budget = GENERATE_TIMEOUT.saturating_sub(started.elapsed());
    if budget.is_zero() {
        return Err(SuggestFailure::Timeout);
    }
    let generated = {
        let llm = state.llm.read().await;
        match &choice {
            ModelChoice::Custom(path) => llm.suggest_custom(&input, path, budget, body).await,
            ModelChoice::Catalog(_) | ModelChoice::Ollama(_) => {
                llm.suggest(&input, &model, budget, body).await
            }
        }
    };
    let memo = match generated {
        Ok(Some(memo)) => memo,
        Ok(None) => return Err(SuggestFailure::Invalid),
        Err(e) => return Err(core_llm::failure_of(e, ollama)),
    };

    // 提案の全文の最後の確認（設計書 10.6）。入力と出力の検査で除かれているはずで、念のため確かめる
    if !memo_sensitive_kinds(&memo_text(&memo)).is_empty() {
        return Err(SuggestFailure::Invalid);
    }
    let mut notices = notices;
    if memo.body_dropped {
        notices.push(SuggestNotice::BodyDropped);
        // 安全のために本文を捨てた結果は保存しない（キャッシュからは、捨てる前の本文を返さないため）
        return Ok(Suggested { memo, notices });
    }
    let store = state.store_clone();
    let stored = memo_text(&memo);
    // 結果は上書きで保存する（作り直しの結果も次回から使う）。保存の失敗は提案の成否に影響させない
    let _ = run_blocking(move || {
        store_message(&store, &key, &model, &stored);
        Ok(())
    })
    .await;
    Ok(Suggested { memo, notices })
}

fn cached_message(store: &Arc<Mutex<Store>>, key: &str) -> Result<Option<String>, AppError> {
    let guard = store.lock().map_err(|_| database_error())?;
    guard.get_cached_message(key).map_err(|_| database_error())
}

fn store_message(store: &Arc<Mutex<Store>>, key: &str, model: &str, message: &str) {
    let Ok(guard) = store.lock() else {
        return;
    };
    let _ = guard.put_cached_message(key, model, message, &now_rfc3339());
}

/// 提案の入力（変更ファイルの一覧と、設定の範囲に応じた差分）を作る。読み取りのみ。
/// 機微なファイル（ファイル名で判定）は内容を読まない（設計書 10.6）。伏せ字と注記は `prepare_input` が作る。
fn collect_input(
    extensions: &ExtensionHost,
    repo: &Path,
    scope: AiScope,
) -> Result<Prepared, AppError> {
    let ops = Ops::new(git_runner());
    let changes = ops.list_changes(repo).map_err(AppError::from_ops_error)?;
    let sources: Vec<SourceChange> = changes
        .iter()
        .take(MAX_INPUT_FILES)
        .map(|change| {
            let sensitive = sensitive_file(&change.path)
                || change.original_path.as_deref().is_some_and(sensitive_file);
            // 「ファイル名まで」は本文を渡さない。「テキスト差分まで」と画像を含む設定は、
            // 画像は本文を渡さず（段階 3 は未実装）、テキストと文書の差分だけを渡す
            let (amount, diff_text) = match scope {
                _ if sensitive => (None, None),
                AiScope::FileNames => (None, None),
                AiScope::TextDiff | AiScope::WithImages => {
                    content_of(&ops, extensions, repo, &change.path)
                }
            };
            SourceChange {
                kind: map_kind(change.kind),
                path: change.path.clone(),
                old_path: change.original_path.clone(),
                amount,
                diff_text,
            }
        })
        .collect();
    Ok(prepare_input(&sources))
}

fn map_kind(kind: ChangedKind) -> ChangeKind {
    match kind {
        ChangedKind::Added => ChangeKind::Added,
        ChangedKind::Modified => ChangeKind::Modified,
        ChangedKind::Deleted => ChangeKind::Deleted,
        ChangedKind::Renamed => ChangeKind::Renamed,
    }
}

/// 1 ファイルの変更量と差分テキスト。docx・xlsx は文書の比較（`compare_office`）を、そのほかはテキストの差分を使う
fn content_of(
    ops: &Ops,
    extensions: &ExtensionHost,
    repo: &Path,
    path: &str,
) -> (Option<String>, Option<String>) {
    match office_kind(path) {
        Some(OfficeKind::Docx | OfficeKind::Xlsx) => office_content(extensions, repo, path),
        Some(OfficeKind::Pptx) | None => text_content(ops, repo, path),
    }
}

fn text_content(ops: &Ops, repo: &Path, path: &str) -> (Option<String>, Option<String>) {
    match ops.memo_text_diff(repo, path, MEMO_TEXT_MAX_BYTES) {
        Ok(Some(MemoTextDiff {
            added,
            removed,
            text,
        })) => (
            Some(text_amount(added, removed)),
            (!text.is_empty()).then_some(text),
        ),
        _ => (None, None),
    }
}

fn office_content(
    extensions: &ExtensionHost,
    repo: &Path,
    path: &str,
) -> (Option<String>, Option<String>) {
    let Ok(diff) = office::compare(extensions, repo, "this", "work", path) else {
        return (None, None);
    };
    match diff {
        office::OfficeDiff::Docx { rows, .. } => {
            let mut added = 0usize;
            let mut removed = 0usize;
            let mut lines = Vec::new();
            for row in &rows {
                match row {
                    ParagraphRow::Added { text, .. } => {
                        added += 1;
                        lines.push(signed_line(true, text));
                    }
                    ParagraphRow::Removed { text, .. } => {
                        removed += 1;
                        lines.push(signed_line(false, text));
                    }
                    ParagraphRow::Context { .. } | ParagraphRow::Omitted { .. } => {}
                }
            }
            (Some(paragraph_amount(added, removed)), join_lines(lines))
        }
        office::OfficeDiff::Xlsx {
            added_sheets,
            removed_sheets,
            cells,
            ..
        } => {
            let mut lines: Vec<String> = Vec::new();
            lines.extend(added_sheets.iter().map(|s| format!("シート追加: {s}")));
            lines.extend(removed_sheets.iter().map(|s| format!("シート削除: {s}")));
            lines.extend(
                cells
                    .iter()
                    .map(|c| cell_line(&c.sheet, &c.cell, c.before.as_deref(), c.after.as_deref())),
            );
            let amount = (!cells.is_empty()).then(|| cell_amount(cells.len()));
            (amount, join_lines(lines))
        }
        office::OfficeDiff::Unavailable { .. } => (None, None),
    }
}

fn join_lines(lines: Vec<String>) -> Option<String> {
    (!lines.is_empty()).then(|| lines.join("\n"))
}

// ========== 接続先・生成の試行 ==========

/// Ollama の一覧（`GET /api/tags`）。失敗は接続の失敗として返す
#[tauri::command]
#[specta::specta]
pub async fn ai_list_ollama_models(url: String) -> Result<Vec<String>, AppError> {
    list_ollama_models(&url).await.map_err(|e| AppError {
        code: "llm_endpoint_failed".to_string(),
        params: Vec::new(),
        what_happened: "接続先からモデルの一覧を取得できませんでした".to_string(),
        data_is_safe: "何も変更されていません".to_string(),
        next_action: "URL と Ollama の起動を確認してください".to_string(),
        technical_info: Some(e.to_string()),
    })
}

/// 接続先の形式と到達性を確認する。形式が正しくないときは `ok` を偽にする（エラーにはしない）。
#[tauri::command]
#[specta::specta]
pub async fn ai_check_endpoint(url: String) -> EndpointCheck {
    match check_endpoint(&url) {
        Err(_) => EndpointCheck {
            ok: false,
            external_host: false,
        },
        Ok(endpoint) => {
            let ok = list_ollama_models(&endpoint.base_url).await.is_ok();
            EndpointCheck {
                ok,
                external_host: endpoint.external_host,
            }
        }
    }
}

/// 固定の短い入力で生成を試す（設定の「試す」）。結果は保存しない。
#[tauri::command]
#[specta::specta]
pub async fn ai_test_generation(app: AppHandle) -> GenerationTest {
    let state = app.state::<AppState>();
    let store = state.store_clone();
    let settings = match run_blocking(move || store_settings(&store)).await {
        Ok(settings) => settings,
        Err(_) => {
            return GenerationTest {
                ok: false,
                reason: Some(TestFailure::Other),
            }
        }
    };
    let Some(choice) = model_choice(&settings) else {
        return failed(TestFailure::NotConfigured);
    };
    if apply_backend(&state.llm, &settings).await.is_err() {
        return failed(TestFailure::InvalidEndpoint);
    }
    let llm = state.llm.read().await;
    let body = settings.ai_suggest_body;
    let result = match &choice {
        ModelChoice::Custom(path) => {
            llm.suggest_custom(&test_input(), path, GENERATE_TIMEOUT, body)
                .await
        }
        ModelChoice::Catalog(_) | ModelChoice::Ollama(_) => {
            llm.suggest(&test_input(), &choice.key(), GENERATE_TIMEOUT, body)
                .await
        }
    };
    match result {
        Err(e) => failed(test_failure_of(e)),
        Ok(None) => failed(TestFailure::Rejected),
        Ok(Some(_)) => GenerationTest {
            ok: true,
            reason: None,
        },
    }
}

fn failed(reason: TestFailure) -> GenerationTest {
    GenerationTest {
        ok: false,
        reason: Some(reason),
    }
}

/// 生成の試行に使う固定の入力
fn test_input() -> ChangeInput {
    prepare_input(&[SourceChange {
        kind: ChangeKind::Modified,
        path: "資料/第3章.docx".to_string(),
        old_path: None,
        amount: Some(paragraph_amount(1, 0)),
        diff_text: Some(format!(
            "{}\n",
            signed_line(true, "実験の回数を5回に修正しました。")
        )),
    }])
    .input
}

fn test_failure_of(e: LlmError) -> TestFailure {
    match e {
        LlmError::ModelNotInstalled | LlmError::UnknownModel => TestFailure::NotInstalled,
        LlmError::InvalidModel => TestFailure::NotConfigured,
        LlmError::CustomModelInvalid => TestFailure::CustomInvalid,
        LlmError::InvalidEndpoint => TestFailure::InvalidEndpoint,
        LlmError::ServerNotFound => TestFailure::ServerNotFound,
        LlmError::ServerStartFailed => TestFailure::ServerStartFailed,
        LlmError::ServerStartTimeout => TestFailure::ServerStartTimeout,
        LlmError::Timeout => TestFailure::Timeout,
        LlmError::Network => TestFailure::Network,
        LlmError::BadStatus => TestFailure::BadStatus,
        LlmError::BadResponse | LlmError::ResponseTooLarge => TestFailure::BadResponse,
        LlmError::Catalog | LlmError::Io => TestFailure::Other,
    }
}

/// アプリの終了時に、内蔵の llama-server を止める
pub(crate) async fn stop_on_exit(state: &AppState) {
    state.llm.read().await.stop().await;
}
