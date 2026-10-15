// 公開データ型（設計書 14.4・14.5・14.8）。画面との間で受け渡すため、serde の形を固定する。

use serde::{Deserialize, Serialize};

/// 1 ファイルの上限（25 MiB）。GitHub の blob と画面メモリの都合による暫定値（設計書 14.5）。
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
/// 1 回の追加（upload）の合計の上限（60 MiB）。
pub const MAX_TOTAL_BYTES: u64 = 60 * 1024 * 1024;
/// 1 回の追加の件数の上限。
pub const MAX_ITEMS: usize = 100;

/// 保存先（リポジトリ）の要約。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSummary {
    /// 所有者（個人または Organization）のログイン名
    pub owner: String,
    /// リポジトリ名
    pub name: String,
    pub private: bool,
    /// 既定ブランチ名
    pub default_branch: String,
    pub pushed_at: Option<String>,
    pub description: Option<String>,
}

/// フォルダの中の項目の種類。サブモジュール・シンボリックリンクは含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
}

/// フォルダの中の 1 項目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    /// リポジトリのルートからのパス
    pub path: String,
    pub kind: EntryKind,
    /// ファイルの大きさ（バイト）。フォルダは 0
    pub size: u64,
    /// blob（ファイル）または tree（フォルダ）の ID
    pub sha: String,
}

/// フォルダの一覧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirListing {
    /// 既定ブランチ名。`ref` を指定したときは、指定された値をそのまま入れる
    pub branch: String,
    /// 一覧の基になったコミットの ID
    pub head_sha: String,
    /// GitHub 側の 1,000 件の上限に達し、一部のみ表示していること
    pub truncated: bool,
    pub entries: Vec<DirEntry>,
}

/// 過去の版（コミット）の 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    pub sha: String,
    /// コミットメッセージの 1 行目だけ
    pub message: String,
    /// 作成日時（RFC3339）
    pub date: String,
    /// 作成者の名前
    pub author: String,
}

/// 追加の計画の入力（同名の確認に使う）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// 保存先のフォルダ（リポジトリのルートからの `/` 区切り。空はルート）
    pub target_dir: String,
    /// ファイル名
    pub name: String,
    /// 大きさ（バイト）
    pub size: u64,
}

/// 追加の計画の 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedItem {
    /// リポジトリのルートからのパス
    pub target_path: String,
    /// 同名のファイルが既にあるか（大文字小文字を区別しない）
    pub exists: bool,
}

/// 追加の計画。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadPlan {
    /// 保存先のブランチ名
    pub branch: String,
    pub items: Vec<PlannedItem>,
}

/// 同名のファイルがあるときの扱い（設計書 14.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnExists {
    /// 既存のファイルを新しい内容で置き換える（古い版は履歴に残る）
    Replace,
    /// `名前 (2).拡張子` の形で、既存と重ならない名前で残す
    KeepBoth,
    /// 追加しない
    Skip,
}

/// 追加する 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadItem {
    pub target_dir: String,
    pub name: String,
    pub content: Vec<u8>,
    pub on_exists: OnExists,
}

/// 追加の要求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadRequest {
    /// 保存メッセージ。None か空白のみのときは既定の文言にする
    pub message: Option<String>,
    pub items: Vec<UploadItem>,
}

/// 名前を変えて保存したファイル（`両方残す` のとき）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renamed {
    /// 要求されたパス
    pub from: String,
    /// 実際に保存されたパス
    pub to: String,
}

/// 追加の結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadResult {
    /// 新しいコミットの ID。何も保存しなかったときは空文字
    pub commit_sha: String,
    /// 保存したパス
    pub saved: Vec<String>,
    /// 追加しなかったパス（`追加しない` を選んだもの）
    pub skipped: Vec<String>,
    pub renamed: Vec<Renamed>,
}
