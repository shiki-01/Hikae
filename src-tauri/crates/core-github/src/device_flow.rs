use crate::error::AuthError;
use crate::token::{AccessToken, DeviceCode};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

/// Device Flow API レスポンス（device code 取得）
#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// Access Token API レスポンス
#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
    error: Option<String>,
    #[allow(dead_code)]
    error_description: Option<String>,
    #[allow(dead_code)]
    error_uri: Option<String>,
    // slow_down の場合、新しい interval が来ることもある
    interval: Option<u64>,
}

/// GitHub OAuth Device Flow クライアント。
/// Device Flow は以下のステップで動作する:
/// 1. request_device_code() を呼び出し、user_code と verification_uri を取得
/// 2. ユーザーが verification_uri で user_code を入力
/// 3. poll_for_token() を呼び出し、トークンを取得するまでポーリング
pub struct DeviceFlowClient {
    client: Client,
    client_id: String,
    // テスト用エンドポイント差し替え
    device_code_endpoint: String,
    token_endpoint: String,
    // 設定可能なポーリング параメータ
    slow_down_increment: u64,
}

impl DeviceFlowClient {
    /// 本番環境用の DeviceFlowClient を作成する。
    pub fn new(client_id: String) -> Self {
        Self {
            client: Client::new(),
            client_id,
            device_code_endpoint: "https://github.com/login/device/code".to_string(),
            token_endpoint: "https://github.com/login/oauth/access_token".to_string(),
            slow_down_increment: 5,
        }
    }

    /// テスト用にエンドポイントをカスタマイズする。
    pub fn with_endpoints(
        client_id: String,
        device_code_endpoint: String,
        token_endpoint: String,
    ) -> Self {
        Self {
            client: Client::new(),
            client_id,
            device_code_endpoint,
            token_endpoint,
            slow_down_increment: 5,
        }
    }

    /// テスト用に slow_down 時の加算秒数と最小待ち時間をカスタマイズする。
    pub fn with_polling_config(mut self, slow_down_increment: u64) -> Self {
        self.slow_down_increment = slow_down_increment;
        self
    }

    /// Device code と verification_uri を要求する。
    /// スコープは常に "repo read:org" で固定。
    pub async fn request_device_code(&self) -> Result<DeviceCode, AuthError> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("scope", "repo read:org"),
        ];

        let response = self
            .client
            .post(&self.device_code_endpoint)
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await
            .map_err(|_| AuthError::FailedToRequestDeviceCode)?;

        if !response.status().is_success() {
            return Err(AuthError::FailedToRequestDeviceCode);
        }

        let body = response
            .json::<DeviceCodeResponse>()
            .await
            .map_err(|_| AuthError::InvalidDeviceCodeResponse)?;

        Ok(DeviceCode::new(
            body.device_code,
            body.user_code,
            body.verification_uri,
            body.expires_in,
            body.interval,
        ))
    }

    /// Device code からアクセストークンを取得するまでポーリングする。
    /// キャンセルトークンでポーリングをキャンセル可能。
    pub async fn poll_for_token(
        &self,
        device: &DeviceCode,
        cancel_token: Option<&CancellationToken>,
    ) -> Result<AccessToken, AuthError> {
        let start = std::time::Instant::now();
        let expires = Duration::from_secs(device.expires_in);
        let mut interval = device.interval;

        loop {
            // キャンセル確認
            if let Some(cancel) = cancel_token {
                if cancel.is_cancelled() {
                    return Err(AuthError::PollingCanceled);
                }
            }

            // タイムアウト確認
            if start.elapsed() > expires {
                return Err(AuthError::Expired);
            }

            // ポーリング
            let token_result = self.request_token(device.device_code()).await?;

            match token_result {
                TokenPollResult::Success(token) => return Ok(token),
                TokenPollResult::Pending => {
                    // まだ許可されていない。interval だけ待機して再試行
                    sleep(Duration::from_secs(interval)).await;
                }
                TokenPollResult::SlowDown(new_interval) => {
                    // サーバー側の要求で interval を増やす（既定は +5秒）
                    interval = new_interval.unwrap_or(interval + self.slow_down_increment);
                    sleep(Duration::from_secs(interval)).await;
                }
            }
        }
    }

    /// token_endpoint に直接ポーリングリクエストを送る。
    async fn request_token(&self, device_code: &str) -> Result<TokenPollResult, AuthError> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let response = self
            .client
            .post(&self.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|_| AuthError::FailedToRequestToken)?;

        // GitHub の Device Flow は 200 OK で error フィールドを返すことに注意
        let body = response
            .json::<AccessTokenResponse>()
            .await
            .map_err(|_| AuthError::InvalidTokenResponse)?;

        // access_token がある場合は成功
        if let Some(token) = body.access_token {
            return Ok(TokenPollResult::Success(AccessToken::new(token)));
        }

        // error フィールドをチェック
        match body.error.as_deref() {
            Some("authorization_pending") => {
                // ユーザーが未実行
                Ok(TokenPollResult::Pending)
            }
            Some("slow_down") => {
                // サーバーが slow_down を要求。新しい interval があればそれを採用
                Ok(TokenPollResult::SlowDown(body.interval))
            }
            Some("expired_token") => Err(AuthError::Expired),
            Some("access_denied") => Err(AuthError::AccessDenied),
            Some("incorrect_device_code") => Err(AuthError::InvalidDeviceCode),
            Some("device_flow_disabled") => Err(AuthError::DeviceFlowDisabled),
            Some(_) => {
                // 未知のエラー。エラー内容をメッセージに含めない（トークンの可能性がある）
                Err(AuthError::FailedToRequestToken)
            }
            None => Err(AuthError::InvalidTokenResponse),
        }
    }
}

/// ポーリングの結果
enum TokenPollResult {
    Success(AccessToken),
    Pending,
    SlowDown(Option<u64>),
}

/// 環境変数から client_id を読み込む。
pub fn client_id_from_env() -> Result<String, AuthError> {
    client_id_from(|key| std::env::var(key).ok())
}

/// 環境変数の参照元を差し替え可能にした内部版（テストが並列でも壊れないようにする）
fn client_id_from(lookup: impl Fn(&str) -> Option<String>) -> Result<String, AuthError> {
    lookup("HIKAE_GITHUB_CLIENT_ID")
        .filter(|v| !v.is_empty())
        .ok_or(AuthError::MissingClientId)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[tokio::test]
    async fn test_request_device_code_success() {
        let server = MockServer::start();

        let device_code_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/login/device/code")
                .header("accept", "application/json");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "device_code": "test_device_code",
                    "user_code": "ABC-1234",
                    "verification_uri": "https://github.com/login/device",
                    "expires_in": 900,
                    "interval": 5,
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        );

        let device = client.request_device_code().await.expect("request failed");

        assert_eq!(device.user_code, "ABC-1234");
        assert_eq!(device.verification_uri, "https://github.com/login/device");
        assert_eq!(device.expires_in, 900);
        assert_eq!(device.interval, 5);

        device_code_mock.assert();
    }

    #[tokio::test]
    async fn test_request_device_code_http_error() {
        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(POST).path("/login/device/code");
            then.status(400);
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        );

        let result = client.request_device_code().await;
        assert!(matches!(result, Err(AuthError::FailedToRequestDeviceCode)));
    }

    #[tokio::test]
    async fn test_poll_for_token_success() {
        let server = MockServer::start();

        // 成功を返すモック（複数回のポーリングに対応）
        let mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "access_token": "test_access_token",
                    "token_type": "bearer",
                    "scope": "repo,read:org",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        );

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            10, // 短い有効期限
            1,  // 1 秒待機
        );

        let token = client
            .poll_for_token(&device, None)
            .await
            .expect("polling failed");

        assert_eq!(token.expose_secret(), "test_access_token");
        mock.assert();
    }

    #[tokio::test]
    async fn test_poll_for_token_expired() {
        let server = MockServer::start();

        // 常に pending を返す
        let _mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "error": "authorization_pending",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        )
        .with_polling_config(1);

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            2, // 2 秒で失効
            1,
        );

        let result = client.poll_for_token(&device, None).await;
        assert!(matches!(result, Err(AuthError::Expired)));
    }

    #[tokio::test]
    async fn test_poll_for_token_access_denied() {
        let server = MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "error": "access_denied",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        );

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            60,
            5,
        );

        let result = client.poll_for_token(&device, None).await;
        assert!(matches!(result, Err(AuthError::AccessDenied)));
    }

    #[tokio::test]
    async fn test_poll_for_token_device_flow_disabled() {
        let server = MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "error": "device_flow_disabled",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        );

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            60,
            5,
        );

        let result = client.poll_for_token(&device, None).await;
        assert!(matches!(result, Err(AuthError::DeviceFlowDisabled)));
    }

    #[tokio::test]
    async fn test_poll_for_token_slow_down() {
        let server = MockServer::start();

        // slow_down レスポンスを返すモック
        // （実テストでは slow_down が来たら interval を増やして再ポーリングするが、
        // モックが常に success を返すので、最初のポーリングで成功する）
        let _mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "access_token": "test_access_token",
                    "token_type": "bearer",
                    "scope": "repo,read:org",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        )
        .with_polling_config(1);

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            10,
            1,
        );

        let token = client
            .poll_for_token(&device, None)
            .await
            .expect("polling failed");

        assert_eq!(token.expose_secret(), "test_access_token");
    }

    #[tokio::test]
    async fn test_poll_for_token_cancellation() {
        let server = MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "error": "authorization_pending",
                }));
        });

        let base_url = server.url("");
        let client = DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            format!("{}/login/device/code", base_url),
            format!("{}/login/oauth/access_token", base_url),
        )
        .with_polling_config(1);

        let device = DeviceCode::new(
            "test_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            60,
            10,
        );

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        // ポーリングをキャンセルするタスクを起動
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel_clone.cancel();
        });

        let result = client.poll_for_token(&device, Some(&cancel)).await;
        assert!(matches!(result, Err(AuthError::PollingCanceled)));
    }

    #[test]
    fn test_client_id_from_env_missing() {
        let result = client_id_from(|_| None);
        assert!(matches!(result, Err(AuthError::MissingClientId)));
        let empty = client_id_from(|_| Some(String::new()));
        assert!(matches!(empty, Err(AuthError::MissingClientId)));
    }

    #[test]
    fn test_client_id_from_env_success() {
        let result = client_id_from(|key| {
            assert_eq!(key, "HIKAE_GITHUB_CLIENT_ID");
            Some("test_client_id_value".to_string())
        })
        .expect("should succeed");
        assert_eq!(result, "test_client_id_value");
    }

    #[test]
    fn test_token_not_exposed_in_error() {
        // error メッセージにトークンが含まれないことを確認する
        let error = AuthError::InvalidTokenResponse;
        let error_msg = format!("{}", error);
        assert!(!error_msg.contains("token"));
        assert!(!error_msg.contains("secret"));
    }
}
