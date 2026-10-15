// モバイル版のリモート操作のエラー。トークンや URL・応答本文は含めない。

use core_github::AuthError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemoteError {
    /// 認証・通信・GitHub 側の障害（`core-github` の分類をそのまま使う）
    #[error("{0}")]
    Auth(#[from] AuthError),

    /// 名前やパスが規則に合わない。理由は短い文字列で入る
    #[error("パスが正しくありません（{0}）")]
    InvalidPath(String),

    /// 1 ファイルまたは 1 回の追加の合計が上限を超えている
    #[error("ファイルが大きすぎます（1 ファイル 25 MiB、1 回の追加は合計 60 MiB まで）")]
    TooLarge,

    /// 件数が上限を超えている。フォルダの中身が 1,000 件を超え、同名の確認ができないときも含む
    #[error("件数が上限を超えています（1 回に 100 件まで。フォルダの中身が多すぎるときは確認できません）")]
    TooMany,

    /// 追加する項目が 1 つもない
    #[error("追加するファイルがありません")]
    EmptyRequest,

    /// 対象が見つからない（空のリポジトリの既定ブランチを含む）
    #[error("見つかりませんでした")]
    NotFound,

    /// 同じ名前のフォルダがあり、ファイルとして追加できない
    #[error("同じ名前のフォルダがあるため追加できません（{0}）")]
    NameIsFolder(String),

    /// 同じ要求の中で、保存先のパスが重複している
    #[error("同じ場所に複数のファイルを追加しようとしています（{0}）")]
    DuplicateTarget(String),

    /// 再試行しても、他の端末の更新と重なり続けた（リモートは変わっていない）
    #[error("他の場所で更新されたため追加できませんでした。もう一度試してください")]
    NotFastForward,

    /// 許可リストに無い呼び出し（内部の不具合の検出）
    #[error("許可されていない GitHub の操作です")]
    Disallowed,

    /// GitHub の応答の形が想定と違う
    #[error("GitHub から想定外の応答がありました")]
    InvalidResponse,
}
