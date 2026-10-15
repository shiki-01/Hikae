// GitHub REST API への通信の入口。HTTP を送るのは `RemoteClient::send` だけ（許可リストを通す）。

use crate::allow::{is_allowed, is_allowed_body, query_string, Call};
use crate::error::RemoteError;
use core_github::{http, AccessToken};
use reqwest::{Client, StatusCode};
use serde_json::Value;

/// 本番の GitHub REST API の基点
const GITHUB_API_BASE: &str = "https://api.github.com";

/// GitHub REST API のクライアント（モバイル版の閲覧とアップロード）。
pub struct RemoteClient {
    base: String,
    http: Client,
}

impl RemoteClient {
    /// 本番の GitHub に接続するクライアント。
    pub fn new() -> Self {
        Self::with_base(GITHUB_API_BASE)
    }

    /// 接続先の基点を指定する（テスト用。末尾の `/` は付けても付けなくてもよい）。
    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: http::client(),
        }
    }

    /// 許可リストを通したうえで 1 回の呼び出しを送り、JSON の応答を返す。
    ///
    /// - 404 と 409 は `NotFound`（409 は空のリポジトリの ref の取得で返る）
    /// - 422 は、ref の更新（`UpdateRef`）のときだけ `NotFastForward`
    /// - それ以外の失敗は `core-github` の分類に従う
    ///
    /// トークンはヘッダにだけ付け、URL・ログ・エラーには出さない。
    pub(crate) async fn send(
        &self,
        token: &AccessToken,
        call: Call<'_>,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<Value, RemoteError> {
        let method = call.method();
        let path = call.path();
        // 送信の直前の検査（Call から作ったものでも、規則を外れていれば送らない）
        if !is_allowed(&method, &path) || !is_allowed_body(&method, &path, body.as_ref()) {
            return Err(RemoteError::Disallowed);
        }
        let url = format!("{}/{}{}", self.base, path, query_string(query));
        let mut request = http::with_github_headers(self.http.request(method, url), token);
        if let Some(body) = &body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| RemoteError::Auth(http::classify_send_error(e)))?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND || status == StatusCode::CONFLICT {
            return Err(RemoteError::NotFound);
        }
        if status == StatusCode::UNPROCESSABLE_ENTITY && matches!(call, Call::UpdateRef { .. }) {
            return Err(RemoteError::NotFastForward);
        }
        if !status.is_success() {
            let remaining = http::rate_limit_remaining(&response);
            return Err(RemoteError::Auth(http::classify_status(
                status,
                remaining.as_deref(),
            )));
        }
        response
            .json::<Value>()
            .await
            .map_err(|_| RemoteError::InvalidResponse)
    }
}

impl Default for RemoteClient {
    fn default() -> Self {
        Self::new()
    }
}

/// 応答の JSON を型に読む。形が違えば `InvalidResponse`。
pub(crate) fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RemoteError> {
    serde_json::from_value(value).map_err(|_| RemoteError::InvalidResponse)
}
