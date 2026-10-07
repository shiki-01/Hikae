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
    /// git エラー
    #[error("git error: {0}")]
    Git(#[from] core_git::GitError),

    /// safety エラー
    #[error("safety error: {0}")]
    Safety(#[from] core_safety::SafetyError),

    /// I/O エラー
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// 競合が未解決のまま操作しようとした
    #[error("merge conflict needs resolution")]
    Conflict(Vec<ConflictFile>),

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
