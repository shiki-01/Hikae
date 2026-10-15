// 画面に返すエラー（設計書 5 章の 3 要素。形はデスクトップの `AppError` と同じ）。
// 画面は `code` と `params` から文言を組み立て、対応が無いときは日本語のフォールバックを使う。
// トークンや認証情報は入れない。技術情報には種類名だけを入れる。

use core_github::AuthError;
use core_remote::RemoteError;
use serde::{Deserialize, Serialize};

/// 読み取りと、失敗した追加のあとでも言える一文（リモートは変更されていない）
pub const REMOTE_UNCHANGED: &str = "GitHub 上のファイルは変更されていません。";
/// 追加が成功したあとの通信で失敗したときの一文
pub const REMOTE_SAFE: &str = "GitHub 上のファイルは無事です。";

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileError {
    /// 機械可読なエラーコード。デスクトップと共通のコード（`not_logged_in` など）を再利用する
    pub code: String,
    /// 文言に差し込む値（キーと値の組。ファイル名など）
    pub params: Vec<(String, String)>,
    /// 何が起きたか
    pub what_happened: String,
    /// データは無事か（1 文で）
    pub data_is_safe: String,
    /// 次の行動
    pub next_action: String,
    /// 技術情報（秘密情報を含まない）
    pub technical_info: Option<String>,
}

impl MobileError {
    fn new(
        code: &str,
        params: Vec<(String, String)>,
        what_happened: &str,
        next_action: &str,
        technical_info: Option<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            params,
            what_happened: what_happened.to_string(),
            data_is_safe: REMOTE_UNCHANGED.to_string(),
            next_action: next_action.to_string(),
            technical_info,
        }
    }

    /// 入力の不備や内部の失敗など、短い説明だけで足りるもの
    pub fn invalid_input(detail: &str) -> Self {
        Self::new(
            "invalid_input",
            Vec::new(),
            "入力の内容が正しくありません。",
            "内容を確かめて、もう一度お試しください",
            Some(detail.to_string()),
        )
    }

    pub fn task_failed(detail: &str) -> Self {
        Self::new(
            "task_failed",
            Vec::new(),
            "処理を完了できませんでした。",
            "もう一度お試しください",
            Some(detail.to_string()),
        )
    }

    pub fn login_page_unexpected() -> Self {
        Self::new(
            "login_page_unexpected",
            Vec::new(),
            "ログインのページを開けませんでした。",
            "表示されているページを、ブラウザで手動で開いてください",
            Some("unexpected verification url".to_string()),
        )
    }

    pub fn login_not_in_progress() -> Self {
        Self::new(
            "login_not_in_progress",
            Vec::new(),
            "ログインの手続きが進行中ではありません。",
            "もう一度ログインを始めてください",
            None,
        )
    }

    pub fn browser_open_failed(detail: String) -> Self {
        Self::new(
            "browser_open_failed",
            Vec::new(),
            "ブラウザを開けませんでした。",
            "表示されているページを、ブラウザで手動で開いてください",
            Some(detail),
        )
    }
}

impl From<AuthError> for MobileError {
    fn from(e: AuthError) -> Self {
        let (code, what, next) = match e {
            AuthError::Unauthorized => (
                "not_logged_in",
                "GitHub との接続が切れました。",
                "もう一度ログインしてください",
            ),
            AuthError::Forbidden => (
                "github_forbidden",
                "GitHub がこの操作を許可しませんでした。Organization の管理者による許可が必要な場合があります。",
                "Organization の管理者に確認してください",
            ),
            AuthError::RateLimited => (
                "github_rate_limited",
                "GitHub への問い合わせが短時間に多すぎました。",
                "しばらく待ってから、もう一度お試しください",
            ),
            AuthError::NetworkUnavailable => (
                "network_unavailable",
                "インターネットに接続できません。",
                "接続を確認してから、もう一度お試しください",
            ),
            AuthError::ServerUnavailable => (
                "github_unavailable",
                "GitHub が一時的に応答していません。",
                "少し時間をおいてから、もう一度お試しください",
            ),
            AuthError::MissingClientId | AuthError::DeviceFlowDisabled => (
                "login_not_configured",
                "GitHub にログインするための設定が見つかりません。",
                "アプリの提供元に連絡してください",
            ),
            AuthError::KeyringError
            | AuthError::FailedToSaveToken
            | AuthError::FailedToLoadToken
            | AuthError::FailedToDeleteToken => (
                "keychain_error",
                "ログイン情報を端末の安全な保管場所とやり取りできませんでした。",
                "もう一度お試しください。解決しない場合は、端末を再起動してからお試しください",
            ),
            AuthError::NoPendingLogin | AuthError::LoginInProgress => (
                "login_not_in_progress",
                "ログインの手続きが進行中ではありません。",
                "もう一度ログインを始めてください",
            ),
            _ => (
                "github_error",
                "GitHub との通信に失敗しました。",
                "しばらくしてから、もう一度お試しください",
            ),
        };
        Self::new(code, Vec::new(), what, next, Some(format!("{e:?}")))
    }
}

impl From<RemoteError> for MobileError {
    fn from(e: RemoteError) -> Self {
        match e {
            RemoteError::Auth(a) => a.into(),
            RemoteError::InvalidPath(reason) => Self::new(
                "invalid_file_name",
                vec![("reason".to_string(), reason.clone())],
                "ファイル名またはフォルダ名に使えない文字や名前が含まれています。",
                "名前を変えて、もう一度お試しください",
                Some(reason),
            ),
            RemoteError::TooLarge => Self::new(
                "remote_file_too_large",
                Vec::new(),
                "ファイルが大きすぎるため、このアプリでは扱えません。",
                "1 ファイル 25 MB まで、1 回の追加は合計 60 MB までです。分けて追加してください",
                None,
            ),
            RemoteError::TooMany => Self::new(
                "remote_too_many_items",
                Vec::new(),
                "一度に追加できるファイルの数を超えています。",
                "100 件以内に分けて、もう一度お試しください",
                None,
            ),
            RemoteError::EmptyRequest => Self::new(
                "invalid_input",
                Vec::new(),
                "追加するファイルがありません。",
                "ファイルやテキストを選んでください",
                None,
            ),
            RemoteError::NotFound => Self::new(
                "remote_not_found",
                Vec::new(),
                "指定された場所が GitHub 上に見つかりませんでした。",
                "一覧に戻って、選び直してください",
                None,
            ),
            RemoteError::NameIsFolder(name) => Self::new(
                "remote_name_is_folder",
                vec![("name".to_string(), name)],
                "同じ名前のフォルダがすでにあります。",
                "ファイルの名前を変えて、もう一度お試しください",
                None,
            ),
            RemoteError::DuplicateTarget(path) => Self::new(
                "remote_duplicate_target",
                vec![("path".to_string(), path)],
                "同じ場所に同じ名前で追加するファイルが 2 つ以上あります。",
                "名前を変えて、もう一度お試しください",
                None,
            ),
            RemoteError::NotFastForward => Self::new(
                "remote_changed_meanwhile",
                Vec::new(),
                "他の場所で更新されたため、追加できませんでした。",
                "もう一度お試しください",
                None,
            ),
            RemoteError::Disallowed | RemoteError::InvalidResponse => Self::new(
                "github_error",
                Vec::new(),
                "GitHub との通信に失敗しました。",
                "しばらくしてから、もう一度お試しください",
                Some(format!("{e:?}")),
            ),
        }
    }
}
