// LLM 機能の公開エラー型。文言は固定の短い日本語のみで、ファイル内容・パス・URL・トークンを含めない。

use thiserror::Error;

/// ローカル LLM 機能のエラー。`Display` は固定文言を返す。
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmError {
    #[error("接続先の URL が正しくありません")]
    InvalidEndpoint,

    #[error("モデルの指定が正しくありません")]
    InvalidModel,

    #[error("モデルがカタログにありません")]
    UnknownModel,

    #[error("モデルが導入されていません")]
    ModelNotInstalled,

    #[error("ローカル LLM の実行ファイルが見つかりません")]
    ServerNotFound,

    #[error("ローカル LLM の起動に失敗しました")]
    ServerStartFailed,

    #[error("ローカル LLM の起動が時間内に完了しませんでした")]
    ServerStartTimeout,

    #[error("提案の生成がタイムアウトしました")]
    Timeout,

    #[error("接続先に接続できませんでした")]
    Network,

    #[error("接続先が正常な応答を返しませんでした")]
    BadStatus,

    #[error("応答が大きすぎます")]
    ResponseTooLarge,

    #[error("応答の形式が正しくありません")]
    BadResponse,

    #[error("モデルカタログの読み込みに失敗しました")]
    Catalog,

    #[error("ファイル操作に失敗しました")]
    Io,

    #[error("自分で用意したモデルファイルが使えません")]
    CustomModelInvalid,
}
