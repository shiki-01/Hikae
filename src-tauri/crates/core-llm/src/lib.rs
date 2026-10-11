// ローカル LLM 統合（llama-server サイドカー、OpenAI 互換 API）。詳細設定で Ollama に切替可能。
// 設計書 10 章。Tauri・git・他の core-* には依存しない（core-preview は宣言のみ）。
//
// - ネットワークへの通信は、モデルのダウンロード（許可リストのホスト）、ユーザーが指定した Ollama、127.0.0.1 の llama-server のみ
// - git は呼ばない。トークン・認証情報は扱わない
// - ファイル内容・パスはログやエラーメッセージに出さない（このクレートはログ出力を持たない）

pub mod catalog;
pub mod client;
pub mod download;
pub mod error;
pub mod input;
pub mod prompt;
pub mod requirements;
pub mod service;
pub mod sidecar;
pub mod suggestion;
pub mod validate;

pub use catalog::{Catalog, CatalogEntry, CatalogError};
pub use client::{
    check_endpoint, list_ollama_models, Backend, ChatClient, ChatOutput, Endpoint, GENERATE_TIMEOUT,
};
pub use download::{installed, is_allowed_url, remove, DownloadError, Downloader, Installed};
pub use error::LlmError;
pub use input::{
    cache_key, cell_amount, cell_line, change_input, paragraph_amount, signed_line, text_amount,
    SourceChange,
};
pub use prompt::{build_messages, ChangeInput, ChangeKind, ChangedFile, Message, Role};
pub use requirements::{required_disk_bytes, stage_availability, Availability, Reason, SystemInfo};
pub use service::{LlmService, ServiceConfig};
pub use sidecar::{Clock, LlamaServer, LlamaServerOptions, SystemClock};
pub use suggestion::{failure_of, SuggestFailure, SuggestSlot};
pub use validate::{clean_summary, clean_summary_detail, Cleaned};
