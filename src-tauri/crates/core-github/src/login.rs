// Device Flow のログイン手続きの状態管理。
//
// 手続き中の device_code はこのモジュールの内部にだけ保持し、画面側には
// 確認用のコード・URL・有効期限・ポーリング間隔だけを渡す。
// 待機は async で行い、`cancel` で待機中でも直ちに中断できる。

use crate::device_flow::DeviceFlowClient;
use crate::error::AuthError;
use crate::token::{AccessToken, DeviceCode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio_util::sync::CancellationToken;

/// 画面に出すログイン手順の情報（device_code は含まない）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPrompt {
    /// GitHub の画面で入力するコード
    pub user_code: String,
    /// コードを入力するページの URL
    pub verification_uri: String,
    /// 有効期限（秒）
    pub expires_in: u64,
    /// ポーリング間隔（秒）
    pub interval: u64,
}

/// ログイン待機の結末
pub enum LoginEnd {
    /// 許可された。取得したトークンは呼び出し側がキーチェーンへ保存する
    Token(AccessToken),
    /// ユーザーが GitHub の画面で拒否した
    Denied,
    /// 有効期限が切れた
    Expired,
    /// キャンセルされた
    Canceled,
}

impl std::fmt::Debug for LoginEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginEnd::Token(_) => f.write_str("Token(***)"),
            LoginEnd::Denied => f.write_str("Denied"),
            LoginEnd::Expired => f.write_str("Expired"),
            LoginEnd::Canceled => f.write_str("Canceled"),
        }
    }
}

struct Pending {
    generation: u64,
    device: DeviceCode,
    cancel: CancellationToken,
    waiting: bool,
}

/// 進行中のログイン手続きを 1 件だけ保持する。
#[derive(Default)]
pub struct LoginCoordinator {
    pending: Mutex<Option<Pending>>,
    generation: AtomicU64,
}

impl LoginCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// ログインを始める。進行中の手続きがあれば中断して置き換える。
    pub async fn start(&self, client: &DeviceFlowClient) -> Result<LoginPrompt, AuthError> {
        self.cancel();
        let device = client.request_device_code().await?;
        let prompt = LoginPrompt {
            user_code: device.user_code.clone(),
            verification_uri: device.verification_uri.clone(),
            expires_in: device.expires_in,
            interval: device.interval,
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.lock() = Some(Pending {
            generation,
            device,
            cancel: CancellationToken::new(),
            waiting: false,
        });
        Ok(prompt)
    }

    /// ユーザーの許可・拒否・期限切れ・キャンセルのいずれかになるまで待つ。
    pub async fn wait(&self, client: &DeviceFlowClient) -> Result<LoginEnd, AuthError> {
        let (generation, device, cancel) = {
            let mut guard = self.lock();
            let pending = guard.as_mut().ok_or(AuthError::NoPendingLogin)?;
            if pending.waiting {
                return Err(AuthError::LoginInProgress);
            }
            pending.waiting = true;
            (
                pending.generation,
                pending.device.clone(),
                pending.cancel.clone(),
            )
        };

        let result = client.poll_for_token(&device, Some(&cancel)).await;

        // 自分の手続きがまだ残っていれば片付ける（別の start で置き換わっていれば触らない）
        {
            let mut guard = self.lock();
            if guard.as_ref().is_some_and(|p| p.generation == generation) {
                *guard = None;
            }
        }

        match result {
            Ok(token) => Ok(LoginEnd::Token(token)),
            Err(AuthError::AccessDenied) => Ok(LoginEnd::Denied),
            Err(AuthError::Expired) => Ok(LoginEnd::Expired),
            Err(AuthError::PollingCanceled) => Ok(LoginEnd::Canceled),
            Err(e) => Err(e),
        }
    }

    /// 進行中の手続きを中断する。待機中なら `wait` が `Canceled` で戻る。
    pub fn cancel(&self) {
        if let Some(pending) = self.lock().take() {
            pending.cancel.cancel();
        }
    }

    /// 手続きが進行中か
    pub fn is_pending(&self) -> bool {
        self.lock().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn client(server: &MockServer) -> DeviceFlowClient {
        DeviceFlowClient::with_endpoints(
            "test_client_id".to_string(),
            server.url("/login/device/code"),
            server.url("/login/oauth/access_token"),
        )
    }

    fn mock_device_code(server: &MockServer, interval: u64, expires_in: u64) {
        server.mock(|when, then| {
            when.method(POST).path("/login/device/code");
            then.status(200).json_body(serde_json::json!({
                "device_code": "secret_device_code",
                "user_code": "ABCD-1234",
                "verification_uri": "https://github.com/login/device",
                "expires_in": expires_in,
                "interval": interval,
            }));
        });
    }

    fn mock_pending(server: &MockServer) {
        server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .json_body(serde_json::json!({ "error": "authorization_pending" }));
        });
    }

    #[tokio::test]
    async fn start_returns_prompt_without_device_code() {
        let server = MockServer::start();
        mock_device_code(&server, 5, 900);
        let login = LoginCoordinator::new();
        let prompt = login.start(&client(&server)).await.expect("start");
        assert_eq!(prompt.user_code, "ABCD-1234");
        assert_eq!(prompt.expires_in, 900);
        assert_eq!(prompt.interval, 5);
        // 画面に渡す型にも、そのデバッグ表示にも device_code は出ない
        assert!(!format!("{prompt:?}").contains("secret_device_code"));
        assert!(login.is_pending());
    }

    #[tokio::test]
    async fn wait_without_start_is_an_error() {
        let server = MockServer::start();
        let login = LoginCoordinator::new();
        let err = login.wait(&client(&server)).await.expect_err("no pending");
        assert!(matches!(err, AuthError::NoPendingLogin));
    }

    #[tokio::test]
    async fn wait_returns_token_and_clears_pending() {
        let server = MockServer::start();
        mock_device_code(&server, 0, 900);
        server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200).json_body(serde_json::json!({
                "access_token": "gho_secret_value", "token_type": "bearer", "scope": "repo"
            }));
        });
        let login = LoginCoordinator::new();
        let c = client(&server);
        login.start(&c).await.expect("start");
        let end = login.wait(&c).await.expect("wait");
        match &end {
            LoginEnd::Token(t) => assert_eq!(t.expose_secret(), "gho_secret_value"),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(!format!("{end:?}").contains("gho_secret_value"));
        assert!(!login.is_pending());
    }

    #[tokio::test]
    async fn wait_maps_denied_and_expired() {
        for (error, expect_denied) in [("access_denied", true), ("expired_token", false)] {
            let server = MockServer::start();
            mock_device_code(&server, 0, 900);
            server.mock(|when, then| {
                when.method(POST).path("/login/oauth/access_token");
                then.status(200)
                    .json_body(serde_json::json!({ "error": error }));
            });
            let login = LoginCoordinator::new();
            let c = client(&server);
            login.start(&c).await.expect("start");
            let end = login.wait(&c).await.expect("wait");
            assert!(
                if expect_denied {
                    matches!(end, LoginEnd::Denied)
                } else {
                    matches!(end, LoginEnd::Expired)
                },
                "{error}"
            );
            assert!(!login.is_pending());
        }
    }

    #[tokio::test]
    async fn cancel_ends_a_waiting_login_quickly() {
        let server = MockServer::start();
        // ポーリング間隔が 1 時間でも、キャンセルで即座に戻る
        mock_device_code(&server, 3600, 7200);
        mock_pending(&server);
        let login = Arc::new(LoginCoordinator::new());
        let c = Arc::new(client(&server));
        login.start(&c).await.expect("start");

        let waiter = {
            let (login, c) = (login.clone(), c.clone());
            tokio::spawn(async move { login.wait(&c).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        login.cancel();
        let end = tokio::time::timeout(Duration::from_secs(30), waiter)
            .await
            .expect("must not hang")
            .expect("join")
            .expect("wait");
        assert!(matches!(end, LoginEnd::Canceled));
        assert!(!login.is_pending());
    }

    #[tokio::test]
    async fn second_wait_while_waiting_is_rejected() {
        let server = MockServer::start();
        mock_device_code(&server, 3600, 7200);
        mock_pending(&server);
        let login = Arc::new(LoginCoordinator::new());
        let c = Arc::new(client(&server));
        login.start(&c).await.expect("start");
        let first = {
            let (login, c) = (login.clone(), c.clone());
            tokio::spawn(async move { login.wait(&c).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        let err = login.wait(&c).await.expect_err("already waiting");
        assert!(matches!(err, AuthError::LoginInProgress));
        login.cancel();
        let _ = first.await;
    }

    #[tokio::test]
    async fn restart_cancels_the_previous_login() {
        let server = MockServer::start();
        mock_device_code(&server, 3600, 7200);
        mock_pending(&server);
        let login = Arc::new(LoginCoordinator::new());
        let c = Arc::new(client(&server));
        login.start(&c).await.expect("start");
        let first = {
            let (login, c) = (login.clone(), c.clone());
            tokio::spawn(async move { login.wait(&c).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        login.start(&c).await.expect("restart");
        let end = tokio::time::timeout(Duration::from_secs(30), first)
            .await
            .expect("must not hang")
            .expect("join")
            .expect("wait");
        assert!(matches!(end, LoginEnd::Canceled));
        // 新しい手続きは残っている（古い待機の後始末で消えない）
        assert!(login.is_pending());
        login.cancel();
    }
}
