use thiserror::Error;

/// GitHub 認証に関するエラー型。トークンの平文をメッセージに含めない。
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("環境変数 HIKAE_GITHUB_CLIENT_ID が設定されていません")]
    MissingClientId,

    #[error("Device code の取得に失敗しました")]
    FailedToRequestDeviceCode,

    #[error("Device code 取得時の HTTP レスポンスが無効です")]
    InvalidDeviceCodeResponse,

    #[error("トークンが失効しました")]
    Expired,

    #[error("ユーザーがアクセスを拒否しました")]
    AccessDenied,

    #[error("device code が無効です")]
    InvalidDeviceCode,

    #[error("Device Flow が無効です。GitHub の OAuth App 設定を確認してください")]
    DeviceFlowDisabled,

    #[error("アクセストークン取得時に予期しないエラーが発生しました")]
    FailedToRequestToken,

    #[error("アクセストークン取得時の HTTP レスポンスが無効です")]
    InvalidTokenResponse,

    #[error("ユーザー情報取得に失敗しました")]
    FailedToFetchUser,

    #[error("ユーザー情報取得時の HTTP レスポンスが無効です")]
    InvalidUserResponse,

    #[error("ポーリングがタイムアウトしました")]
    PollingTimeout,

    #[error("ポーリングがキャンセルされました")]
    PollingCanceled,

    #[error("HTTP クライアント エラー")]
    HttpError,

    #[error("トークン保存に失敗しました")]
    FailedToSaveToken,

    #[error("トークン読み込みに失敗しました")]
    FailedToLoadToken,

    #[error("トークン削除に失敗しました")]
    FailedToDeleteToken,

    #[error("JSON 解析エラー")]
    JsonError,

    #[error("キーチェーン エラー")]
    KeyringError,
}
