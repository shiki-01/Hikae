// モバイル版の GitHub REST API クライアント（設計書 14 章）。
//
// 端末にリポジトリを持たず、GitHub の REST API（Contents と Git Data）だけで閲覧と追加を行う。
// Tauri と git（CLI）に依存しない。HTTP の呼び出しは `client.rs` の `send` だけを通り、
// 許可リスト（`allow.rs`）の外の呼び出しは実行時に拒否する。

mod allow;
mod client;
mod encode;
mod error;
mod model;
mod path;
pub mod preview;
mod read;
mod upload;

pub use client::RemoteClient;
pub use error::RemoteError;
pub use model::{
    DirEntry, DirListing, EntryKind, OnExists, PlanItem, PlannedItem, Renamed, RepoSummary,
    UploadItem, UploadPlan, UploadRequest, UploadResult, VersionInfo, MAX_FILE_BYTES, MAX_ITEMS,
    MAX_TOTAL_BYTES,
};
