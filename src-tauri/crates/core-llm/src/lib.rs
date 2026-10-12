// ローカル LLM 統合（llama-server サイドカー、OpenAI 互換 API）。詳細設定で Ollama に切替可能。
// 設計書 10 章。Tauri・git・他の core-* には依存しない（core-preview は宣言のみ）。
//
// - ネットワークへの通信は、モデルのダウンロード（許可リストのホスト）、ユーザーが指定した Ollama、127.0.0.1 の llama-server のみ
// - git は呼ばない。トークン・認証情報は扱わない
// - ファイル内容・パスはログやエラーメッセージに出さない（このクレートはログ出力を持たない）

pub mod catalog;
pub mod choice;
pub mod client;
pub mod custom;
pub mod download;
pub mod error;
pub mod input;
pub mod prompt;
pub mod redact;
pub mod requirements;
pub mod service;
pub mod sidecar;
pub mod suggestion;
pub mod validate;

pub use catalog::{Catalog, CatalogEntry, CatalogError};
pub use choice::ModelChoice;
pub use client::{
    check_endpoint, list_ollama_models, Backend, ChatClient, ChatOutput, Endpoint, GENERATE_TIMEOUT,
};
pub use custom::{custom_model_id, validate_custom_gguf, CustomGguf, CustomGgufError};
pub use download::{installed, is_allowed_url, remove, DownloadError, Downloader, Installed};
pub use error::LlmError;
pub use input::{
    cache_key, cell_amount, cell_line, paragraph_amount, prepare_input, signed_line, text_amount,
    Prepared, SourceChange,
};
pub use prompt::{build_messages, ChangeInput, ChangeKind, ChangedFile, Message, Role};
pub use redact::{
    copies_diff, looks_sensitive, mask_secrets, memo_sensitive_kinds, sensitive_file,
    SensitiveKind, MASK,
};
pub use requirements::{required_disk_bytes, stage_availability, Availability, Reason, SystemInfo};
pub use service::{content_retry_messages, needs_content_retry, LlmService, ServiceConfig};
pub use sidecar::{Clock, LlamaServer, LlamaServerOptions, SystemClock};
pub use suggestion::{failure_of, SuggestFailure, SuggestNotice, SuggestSlot};
pub use validate::{memo_from_text, memo_text, parse_memo, screen_memo, Memo};
