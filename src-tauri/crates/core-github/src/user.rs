use crate::error::AuthError;
use crate::token::{AccessToken, User};
use reqwest::Client;
use serde::Deserialize;

/// GitHub API から取得するユーザー情報の応答
#[derive(Deserialize)]
struct UserResponse {
    id: u64,
    login: String,
}

/// GitHub REST API v3 のユーザー情報エンドポイントにアクセスするクライアント。
pub struct UserClient {
    client: Client,
    user_endpoint: String,
}

impl UserClient {
    /// 本番環境用のクライアントを作成する。
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            user_endpoint: "https://api.github.com/user".to_string(),
        }
    }

    /// テスト用にエンドポイントをカスタマイズする。
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            client: Client::new(),
            user_endpoint: endpoint,
        }
    }

    /// アクセストークンを使用してログイン中のユーザー情報を取得する。
    /// User-Agent は "Hikae" で固定。
    pub async fn fetch_user(&self, token: &AccessToken) -> Result<User, AuthError> {
        let response = self
            .client
            .get(&self.user_endpoint)
            .header("Authorization", format!("Bearer {}", token.expose_secret()))
            .header("User-Agent", "Hikae")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|_| AuthError::FailedToFetchUser)?;

        if !response.status().is_success() {
            return Err(AuthError::FailedToFetchUser);
        }

        let body = response
            .json::<UserResponse>()
            .await
            .map_err(|_| AuthError::InvalidUserResponse)?;

        Ok(User::new(body.id, body.login))
    }
}

impl Default for UserClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[tokio::test]
    async fn test_fetch_user_success() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/user")
                .header("user-agent", "Hikae")
                .header("accept", "application/vnd.github+json");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "id": 12345,
                    "login": "alice",
                }));
        });

        let client = UserClient::with_endpoint(format!("{}/user", server.url("")));
        let token = AccessToken::new("test_token_value".to_string());

        let user = client.fetch_user(&token).await.expect("fetch failed");

        assert_eq!(user.id, 12345);
        assert_eq!(user.login, "alice");
        assert_eq!(user.noreply_email(), "12345+alice@users.noreply.github.com");

        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_user_unauthorized() {
        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(GET).path("/user");
            then.status(401);
        });

        let client = UserClient::with_endpoint(format!("{}/user", server.url("")));
        let token = AccessToken::new("invalid_token".to_string());

        let result = client.fetch_user(&token).await;
        assert!(matches!(result, Err(AuthError::FailedToFetchUser)));
    }

    #[tokio::test]
    async fn test_fetch_user_invalid_response() {
        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(GET).path("/user");
            then.status(200)
                .header("content-type", "application/json")
                .body("{ invalid json");
        });

        let client = UserClient::with_endpoint(format!("{}/user", server.url("")));
        let token = AccessToken::new("test_token".to_string());

        let result = client.fetch_user(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidUserResponse)));
    }
}
