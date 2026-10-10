// 生成の呼び出し（OpenAI 互換 API）と接続先の検査（設計 10.5）。
// 内蔵の llama-server と Ollama は同じ `POST {base}/v1/chat/completions` で呼ぶ。分岐は接続先とモデル一覧の取得だけ。
//
// - 接続先は http / https のみ。ユーザー情報・クエリ・フラグメントを含むものは拒否する
// - ループバック以外のホストは拒否せず、`external_host` を立てて呼び出し側が警告できるようにする
// - 応答は 1 MiB を上限に読む。リダイレクトは追わない（別のホストへ本文を送らないため）

use crate::error::LlmError;
use crate::prompt::Message;
use reqwest::header::CONTENT_TYPE;
use reqwest::{redirect, Client, Response, Url};
use serde::Deserialize;
use std::net::IpAddr;
use std::time::Duration;

/// 応答本文の上限（バイト）
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// 生成の既定タイムアウト（設計 10.2）
pub const DEFAULT_GENERATE_TIMEOUT: Duration = Duration::from_secs(30);

const TEMPERATURE: f64 = 0.2;
const TOP_P: f64 = 0.9;
const MAX_TOKENS: u32 = 100;

/// 生成の接続先
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// 内蔵の llama-server（サイドカーが 127.0.0.1 で起動する）
    Builtin,
    /// Ollama（詳細設定で指定された接続先）
    Ollama { base_url: String },
}

/// 検査済みの接続先
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// 末尾の `/` を除いた基底 URL
    pub base_url: String,
    /// ループバック以外のホストかどうか（呼び出し側は警告を出す）
    pub external_host: bool,
}

/// 接続先 URL を検査する。拒否はしない（外部ホストは `external_host` で知らせる）。
pub fn check_endpoint(raw: &str) -> Result<Endpoint, LlmError> {
    let url = Url::parse(raw.trim()).map_err(|_| LlmError::InvalidEndpoint)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(LlmError::InvalidEndpoint);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(LlmError::InvalidEndpoint);
    }
    if url.query().is_some() || url.fragment().is_some() || url.host_str().is_none() {
        return Err(LlmError::InvalidEndpoint);
    }
    Ok(Endpoint {
        base_url: url.as_str().trim_end_matches('/').to_string(),
        external_host: !is_loopback(&url),
    })
}

/// ホストがループバック（localhost、127.0.0.0/8、::1）か。IPv4 射影の IPv6 も含む。
fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_loopback(),
        Ok(IpAddr::V6(v6)) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

/// HTTP クライアントの作成。プロキシは使わない（接続先はローカルか、利用者が指定したホストに限るため）。
pub(crate) fn http_client() -> Result<Client, LlmError> {
    Client::builder()
        .no_proxy()
        .redirect(redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| LlmError::Network)
}

/// 応答本文を上限つきで読む。
pub(crate) async fn read_limited(mut response: Response) -> Result<Vec<u8>, LlmError> {
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
    {
        return Err(LlmError::ResponseTooLarge);
    }
    let mut buf = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(map_request_error)?;
        let Some(bytes) = chunk else { break };
        if buf.len() + bytes.len() > MAX_RESPONSE_BYTES {
            return Err(LlmError::ResponseTooLarge);
        }
        buf.extend_from_slice(&bytes);
    }
    Ok(buf)
}

fn map_request_error(e: reqwest::Error) -> LlmError {
    if e.is_timeout() {
        LlmError::Timeout
    } else {
        LlmError::Network
    }
}

/// OpenAI 互換 API の呼び出し。
pub struct ChatClient {
    http: Client,
}

impl ChatClient {
    pub fn new() -> Result<Self, LlmError> {
        Ok(Self {
            http: http_client()?,
        })
    }

    /// `POST {base}/v1/chat/completions` を呼び、最初の選択肢の本文を返す。
    /// 生成条件は設計 10.4 と評価レポートに従う（temperature 0.2、top_p 0.9、max_tokens 100、stream なし、thinking 無効）。
    pub async fn chat(
        &self,
        base_url: &str,
        model: &str,
        messages: &[Message],
        timeout: Duration,
    ) -> Result<String, LlmError> {
        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "temperature": TEMPERATURE,
            "top_p": TOP_P,
            "max_tokens": MAX_TOKENS,
            "stream": false,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let bytes = serde_json::to_vec(&body).map_err(|_| LlmError::BadResponse)?;
        let response = self
            .http
            .post(format!("{base_url}/v1/chat/completions"))
            .timeout(timeout)
            .header(CONTENT_TYPE, "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(map_request_error)?;
        if !response.status().is_success() {
            return Err(LlmError::BadStatus);
        }
        let raw = read_limited(response).await?;
        let parsed: ChatResponse =
            serde_json::from_slice(&raw).map_err(|_| LlmError::BadResponse)?;
        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .and_then(|m| m.content)
            .ok_or(LlmError::BadResponse)
    }
}

/// Ollama に導入されているモデル名の一覧（`GET {base}/api/tags`）。
pub async fn list_ollama_models(base_url: &str) -> Result<Vec<String>, LlmError> {
    let endpoint = check_endpoint(base_url)?;
    let http = http_client()?;
    let response = http
        .get(format!("{}/api/tags", endpoint.base_url))
        .timeout(DEFAULT_GENERATE_TIMEOUT)
        .send()
        .await
        .map_err(map_request_error)?;
    if !response.status().is_success() {
        return Err(LlmError::BadStatus);
    }
    let raw = read_limited(response).await?;
    let parsed: TagsResponse = serde_json::from_slice(&raw).map_err(|_| LlmError::BadResponse)?;
    Ok(parsed.models.into_iter().map(|m| m.name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_without_external_flag() {
        for raw in [
            "http://localhost:11434",
            "http://LOCALHOST:11434/",
            "http://127.0.0.1:11434",
            "http://127.5.6.7",
            "http://[::1]:11434",
            "http://[::ffff:127.0.0.1]:11434",
        ] {
            let e = check_endpoint(raw).unwrap();
            assert!(!e.external_host, "{raw}");
        }
    }

    #[test]
    fn external_host_is_flagged_but_allowed() {
        for raw in [
            "http://192.168.1.10:11434",
            "https://ollama.example.com",
            "http://[2001:db8::1]:1",
        ] {
            let e = check_endpoint(raw).unwrap();
            assert!(e.external_host, "{raw}");
        }
    }

    #[test]
    fn strips_trailing_slash() {
        let e = check_endpoint("http://localhost:11434/").unwrap();
        assert_eq!(e.base_url, "http://localhost:11434");
    }

    #[test]
    fn rejects_bad_scheme() {
        for raw in [
            "file:///C:/x",
            "ftp://localhost",
            "javascript:alert(1)",
            "localhost:11434",
        ] {
            assert_eq!(check_endpoint(raw), Err(LlmError::InvalidEndpoint), "{raw}");
        }
    }

    #[test]
    fn rejects_user_info_query_and_fragment() {
        for raw in [
            "http://user:pass@localhost:11434",
            "http://user@localhost:11434",
            "http://localhost:11434/?k=v",
            "http://localhost:11434/#x",
        ] {
            assert_eq!(check_endpoint(raw), Err(LlmError::InvalidEndpoint), "{raw}");
        }
    }

    #[test]
    fn error_text_does_not_contain_input() {
        let raw = "http://secret-user:secret-pass@example.com";
        let err = check_endpoint(raw).unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
