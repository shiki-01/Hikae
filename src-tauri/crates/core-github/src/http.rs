// HTTP クライアントの共通部品。通信エラーや状態コードを、秘密を含まない `AuthError` に分類する。

use crate::error::AuthError;
use crate::token::AccessToken;
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::{Client, RequestBuilder, StatusCode};
use std::time::Duration;

/// 応答が止まったままにならないよう、接続と全体にタイムアウトを付けたクライアントを作る。
pub fn client() -> Client {
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30));
    // Android では OS の証明書の検証（rustls-platform-verifier）に JNI の初期化が要り、
    // 初期化していないと最初の HTTPS 通信で panic する。同梱のルート証明書で検証して避ける
    #[cfg(target_os = "android")]
    let builder = builder.use_preconfigured_tls(android_tls_config());
    builder.build().unwrap_or_else(|_| Client::new())
}

/// Android 用の TLS 設定。Mozilla のルート証明書（webpki-roots）で GitHub の証明書を検証する
#[cfg(target_os = "android")]
fn android_tls_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// 送信前の失敗（接続できない、DNS、タイムアウトなど）。本文や URL は含めない。
pub fn classify_send_error(_e: reqwest::Error) -> AuthError {
    AuthError::NetworkUnavailable
}

/// 成功以外の状態コードを分類する。`rate_limit_remaining` は `x-ratelimit-remaining` ヘッダの値。
pub fn classify_status(status: StatusCode, rate_limit_remaining: Option<&str>) -> AuthError {
    match status.as_u16() {
        401 => AuthError::Unauthorized,
        403 if rate_limit_remaining == Some("0") => AuthError::RateLimited,
        403 => AuthError::Forbidden,
        429 => AuthError::RateLimited,
        500..=599 => AuthError::ServerUnavailable,
        _ => AuthError::UnexpectedStatus,
    }
}

/// 応答ヘッダから `x-ratelimit-remaining` を読む
pub fn rate_limit_remaining(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// GitHub API 用の共通ヘッダ（User-Agent、Accept、Bearer 認証）を付ける。
/// 認証ヘッダは sensitive にして、デバッグ出力にも値が出ないようにする。
pub fn with_github_headers(builder: RequestBuilder, token: &AccessToken) -> RequestBuilder {
    let mut builder = builder
        .header("User-Agent", "Hikae")
        .header("Accept", "application/vnd.github+json");
    if let Ok(mut value) = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret())) {
        value.set_sensitive(true);
        builder = builder.header(AUTHORIZATION, value);
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_status_codes() {
        let s = |c: u16| StatusCode::from_u16(c).unwrap_or(StatusCode::IM_A_TEAPOT);
        assert!(matches!(
            classify_status(s(401), None),
            AuthError::Unauthorized
        ));
        assert!(matches!(
            classify_status(s(403), None),
            AuthError::Forbidden
        ));
        assert!(matches!(
            classify_status(s(403), Some("0")),
            AuthError::RateLimited
        ));
        assert!(matches!(
            classify_status(s(429), None),
            AuthError::RateLimited
        ));
        assert!(matches!(
            classify_status(s(502), None),
            AuthError::ServerUnavailable
        ));
        assert!(matches!(
            classify_status(s(418), None),
            AuthError::UnexpectedStatus
        ));
    }
}
