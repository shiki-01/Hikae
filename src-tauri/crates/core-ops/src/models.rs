// 公開 API で使用するデータ型。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// GitHub ユーザー ID（noreply 前提）
#[derive(Debug, Clone)]
pub struct Identity {
    pub name: String,  // GitHub ユーザー名
    pub email: String, // <id>+<login>@users.noreply.github.com
}

/// 復元点の情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePointInfo {
    /// スナップショット ref 名
    pub snapshot_ref: Option<String>,
    /// バックアップ ref 名
    pub backup_ref: Option<String>,
}

/// 保存の結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SaveOutcome {
    /// 変更がなかった
    NothingToSave,
    /// 保存成功
    Saved {
        commit: String,
        restore_point: RestorePointInfo,
    },
}

/// 取り込みの結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PullOutcome {
    /// upstream との差分がない
    UpToDate,
    /// fast-forward
    FastForwarded,
    /// merge 成功（競合なし）
    Merged { commit: String },
    /// 競合あり
    Conflicted { files: Vec<ConflictFile> },
    /// upstream が未設定
    NoUpstream,
}

/// アップロードの結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UploadOutcome {
    /// push 成功
    Pushed,
    /// アップロード待ちなし
    NothingToUpload,
    /// pull 後 push 成功
    PulledThenPushed(PullOutcome),
    /// pull で競合。解消待ち。
    NeedsResolve(Vec<ConflictFile>),
}

/// 競合ファイルの情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    pub kind: ConflictKind,
    /// この PC 側でそのファイルが最後に保存された時刻（Unix 秒）。取得できなければ None
    pub this_saved_at: Option<i64>,
    /// クラウド側でそのファイルが最後に保存された時刻（Unix 秒）。取得できなければ None
    pub cloud_saved_at: Option<i64>,
}

/// 競合の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictKind {
    /// 双方変更
    BothModified,
    /// 双方追加
    BothAdded,
    /// この PC で削除、相手で変更
    DeletedByUs,
    /// 相手で削除、この PC で変更
    DeletedByThem,
    /// 双方削除
    BothDeleted,
}

/// 競合解消時の選択
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Choice {
    /// この PC の版を採用（ours）
    Mine,
    /// 相手の版を採用（theirs）
    Theirs,
}

/// 競合解消の結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveOutcome {
    /// merge commit のハッシュ
    pub commit: String,
    /// keep_other_copy で保存した別名ファイルのパス
    pub copies: Vec<String>,
}

/// 同期状態
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// このブランチが upstream より前のコミット数
    pub ahead: u32,
    /// このブランチが upstream より後のコミット数
    pub behind: u32,
    /// upstream が設定されているか
    pub has_upstream: bool,
    /// 未保存の変更があるか
    pub dirty: bool,
}

/// 高層操作のエラー型
#[derive(Error, Debug)]
pub enum OpsError {
    /// git エラー（共有違反は `FileInUse` に分類されるため、ここには来ない）
    #[error("git error: {0}")]
    Git(core_git::GitError),

    /// safety エラー
    #[error("safety error: {0}")]
    Safety(core_safety::SafetyError),

    /// I/O エラー
    #[error("io error: {0}")]
    Io(std::io::Error),

    /// ファイルが他のアプリで使用中のため、書き換え・削除できなかった（設計書 5章 E12）
    #[error("file is in use by another application")]
    FileInUse {
        /// 特定できたファイル名（パスは含めない）
        file: Option<String>,
    },

    /// 競合が未解決のまま操作しようとした
    #[error("merge conflict needs resolution")]
    Conflict(Vec<ConflictFile>),

    /// 呼び出し側が渡した値（パス・コミット・復元点など）が不正
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// 予期しないエラー
    #[error("unexpected error: {0}")]
    Unexpected(String),
}

/// 履歴メモや別名コピーに使う文言。UI の言語リソースから呼び出し側が渡す。
/// 既定値は英語（日本語などの表示文言は i18n 側で決め、ここには直書きしない）。
#[derive(Debug, Clone)]
pub struct Labels {
    /// 取り込み前に未保存の変更を自動で保存するときのメモ
    pub auto_save_memo: String,
    /// 「この PC の版」を別名コピーするときにファイル名へ付ける語
    pub copy_mine: String,
    /// 「クラウドの版」を別名コピーするときにファイル名へ付ける語
    pub copy_theirs: String,
}

impl Default for Labels {
    fn default() -> Self {
        Labels {
            auto_save_memo: "Auto-save before pulling".to_string(),
            copy_mine: "this PC".to_string(),
            copy_theirs: "cloud".to_string(),
        }
    }
}

/// 履歴エントリの種別
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HistoryKind {
    /// 利用者が保存した時点
    Manual,
    /// アプリが自動で控えた時点（`refs/hikae/snapshots/` の復元点）
    Auto,
}

/// 履歴の1つのエントリ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// commit のハッシュ（短縮形）
    pub commit: String,
    /// タイムスタンプ（ISO 8601）
    pub timestamp: String,
    /// コミットメッセージ
    pub message: String,
    /// `refs/hikae/snapshots/` に該当する自動保存の ref（None なら手動の保存）
    pub snapshot_ref: Option<String>,
    /// このコミットで変更されたファイル数
    pub changed_files_count: u32,
    /// 手動の保存か自動保存か（`snapshot_ref` の有無と常に対応する）
    pub kind: HistoryKind,
}

/// 特定時点のファイル情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInHistory {
    pub path: String,
    pub mode: String, // "100644" など
    pub size: u64,
    pub is_tracked: bool,
}

/// 元に戻す操作のプレビュー（影響ファイル一覧）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePreview {
    /// 変更されるファイル（いまの内容 → 復元後の内容）
    pub modified: Vec<RestoreFileChange>,
    /// 削除されるファイル
    pub deleted: Vec<String>,
    /// 復活するファイル
    pub created: Vec<String>,
}

/// 復元時のファイルの変更情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreFileChange {
    pub path: String,
    pub size_from: u64,
    pub size_to: u64,
}

/// 1 ファイルを戻すときの影響の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RestoreFileKind {
    /// いまのファイルを指定時点の内容で置き換える
    Overwrite,
    /// いまは無いファイルを指定時点の内容で作り直す
    Recreate,
    /// すでに指定時点と同じ内容（変更なし）
    Unchanged,
    /// 指定時点には存在しないため戻せない
    NotInThatPoint,
}

/// 1 ファイルを戻す操作のプレビュー
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreFilePreview {
    pub kind: RestoreFileKind,
    /// いまのファイルのサイズ（無ければ None）
    pub size_now: Option<u64>,
    /// 指定時点のファイルのサイズ（存在しなければ None）
    pub size_then: Option<u64>,
}

/// 1 ファイルを戻した結果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RestoreFileOutcome {
    /// 戻した。`undo_ref` は取り消し用の復元点（`refs/hikae/` 配下の ref 名）
    Restored { undo_ref: Option<String> },
    /// 指定時点にそのファイルが無いため、何も変更しなかった
    NotInThatPoint,
    /// 同名の「保存対象外」ファイルが作業フォルダにあり、上書きすると失われるため何も変更しなかった
    IgnoredFileInTheWay,
}

/// 保存時点で変更されたファイルの種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PointChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// 保存時点で変更されたファイル（直前の保存との差）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointChange {
    pub path: String,
    /// 名前変更の場合の元のパス
    pub old_path: Option<String>,
    pub kind: PointChangeKind,
}

/// 追加（コピー）できたファイル
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddedFile {
    /// プロジェクトからの相対パス（`/` 区切り）
    pub path: String,
    /// 同名ファイルがあったため別名にした
    pub renamed: bool,
    /// 警告閾値（50MB）以上の大きいファイル
    pub large: bool,
}

/// 追加しなかった理由
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AddRejectReason {
    /// 100MB を超えるため追加できない（`size` は元のサイズ）
    TooLarge { size: u64 },
    /// 通常のファイルではない（フォルダなど）
    NotAFile,
    /// 読み取れない、またはコピーに失敗した
    Unreadable,
}

/// 追加しなかったファイル
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RejectedFile {
    /// 元のファイル名（パスは含めない）
    pub name: String,
    pub reason: AddRejectReason,
}

/// ファイル追加の結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddFilesOutcome {
    pub added: Vec<AddedFile>,
    pub rejected: Vec<RejectedFile>,
}

/// フォルダの付け替え先の確認結果
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelocateCheck {
    /// 同じプロジェクトと確認できた
    Same,
    /// フォルダが無い、またはプロジェクトのフォルダ直下ではない
    NotARepository,
    /// 別のプロジェクトのフォルダ
    DifferentRepository,
    /// 照合できる情報（保存先の URL も初期の保存の記録も）が登録されていないため確認できない
    CannotVerify,
}

/// 差分の行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub line_number_old: Option<u32>,
    pub line_number_new: Option<u32>,
    pub content: String,
}

/// 差分行の種別
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffLineKind {
    /// コンテキスト行
    Context,
    /// 追加行
    Added,
    /// 削除行
    Removed,
}

impl From<core_git::GitError> for OpsError {
    fn from(e: core_git::GitError) -> Self {
        classify_git_error(e)
    }
}

impl From<core_safety::SafetyError> for OpsError {
    fn from(e: core_safety::SafetyError) -> Self {
        match e {
            // 復元点の作成中に起きた共有違反も、同じ種別にそろえる
            core_safety::SafetyError::Git(g) => classify_git_error(g),
            core_safety::SafetyError::Io(io) => OpsError::from(io),
            other => OpsError::Safety(other),
        }
    }
}

impl From<std::io::Error> for OpsError {
    fn from(e: std::io::Error) -> Self {
        if crate::file_in_use::is_sharing_violation(&e) {
            OpsError::FileInUse { file: None }
        } else {
            OpsError::Io(e)
        }
    }
}

/// git の失敗のうち、他のアプリがファイルを使用中のものを `FileInUse` に分類する。
fn classify_git_error(e: core_git::GitError) -> OpsError {
    if let core_git::GitError::Failed { stderr, .. } = &e {
        if crate::file_in_use::is_file_in_use_message(stderr) {
            return OpsError::FileInUse {
                file: crate::file_in_use::file_name_in_message(stderr),
            };
        }
    }
    OpsError::Git(e)
}

impl OpsError {
    /// ジャーナルに記録するための分類名。エラー本文（git の stderr など）は含めない。
    /// stderr にはリモート URL などが含まれうるため、記録には種類だけを使う。
    pub fn kind(&self) -> &'static str {
        match self {
            OpsError::Git(core_git::GitError::Timeout { .. }) => "git-timeout",
            OpsError::Git(_) => "git",
            OpsError::Safety(_) => "safety",
            OpsError::Io(_) => "io",
            OpsError::FileInUse { .. } => "file-in-use",
            OpsError::Conflict(_) => "conflict",
            OpsError::InvalidInput(_) => "invalid-input",
            OpsError::Unexpected(_) => "unexpected",
        }
    }
}

/// 操作の前後の復元点 ref の一覧を比べ、その操作が新しく作った復元点を返す。
/// 同じ種類が複数あれば、名前（時刻入り）が最大のものを選ぶ。
pub fn new_restore_points(before: &[String], after: &[String]) -> RestorePointInfo {
    let created = |prefix: &str| {
        after
            .iter()
            .filter(|r| r.starts_with(prefix) && !before.contains(r))
            .max()
            .cloned()
    };
    RestorePointInfo {
        snapshot_ref: created("refs/hikae/snapshots/"),
        backup_ref: created("refs/hikae/backup/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_restore_points_returns_only_refs_created_by_the_operation() {
        let before = vec![
            "refs/hikae/backup/save/20260101T000000Z".to_string(),
            "refs/hikae/snapshots/main/20260101T000000Z".to_string(),
        ];
        let mut after = before.clone();
        after.push("refs/hikae/backup/pull/20260102T000000Z".to_string());
        after.push("refs/hikae/backup/pull/20260102T000500Z".to_string());
        let info = new_restore_points(&before, &after);
        assert_eq!(info.snapshot_ref, None);
        assert_eq!(
            info.backup_ref.as_deref(),
            Some("refs/hikae/backup/pull/20260102T000500Z")
        );
        assert_eq!(new_restore_points(&after, &after).backup_ref, None);
    }

    #[test]
    fn error_kind_does_not_leak_message() {
        let e = OpsError::Unexpected("token ghp_secret".to_string());
        assert_eq!(e.kind(), "unexpected");
        assert_eq!(OpsError::Conflict(vec![]).kind(), "conflict");
    }
}
