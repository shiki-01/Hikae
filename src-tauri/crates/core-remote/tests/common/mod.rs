// 統合テストの共通部品。GitHub の REST API は httpmock で置き換える。

#![allow(dead_code)]

use core_github::{AccessToken, User};
use core_remote::RemoteClient;
use httpmock::prelude::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub const TOKEN: &str = "test_token_value";
/// 既定ブランチ（main）の先頭コミット
pub const HEAD: &str = "1111111111111111111111111111111111111111";
/// HEAD のツリー
pub const TREE: &str = "2222222222222222222222222222222222222222";
/// 作成される新しいツリー
pub const NEW_TREE: &str = "3333333333333333333333333333333333333333";
/// 作成される新しいコミット
pub const NEW_COMMIT: &str = "4444444444444444444444444444444444444444";

/// 受け取った JSON 本文を記録する
pub type Bodies = Arc<Mutex<Vec<Value>>>;

pub fn bodies() -> Bodies {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn token() -> AccessToken {
    AccessToken::new(TOKEN.to_string())
}

pub fn user() -> User {
    User::new(12345, "alice".to_string())
}

pub fn client(server: &MockServer) -> RemoteClient {
    RemoteClient::with_base(server.url(""))
}

/// `GET /repos/{o}/{n}` の応答
pub fn repo_json(owner: &str, name: &str, default_branch: &str) -> Value {
    json!({
        "name": name,
        "owner": {"login": owner},
        "private": true,
        "default_branch": default_branch,
        "pushed_at": "2026-10-01T00:00:00Z",
        "description": "memo",
    })
}

/// `GET .../git/ref/heads/{b}` の応答
pub fn ref_json(sha: &str) -> Value {
    json!({"ref": "refs/heads/main", "object": {"sha": sha, "type": "commit"}})
}

/// 受け取った本文を記録し、固定の応答を返す
pub fn recording(
    bodies: Bodies,
    status: u16,
    reply: Value,
) -> impl Fn(&HttpMockRequest) -> HttpMockResponse + Send + Sync + 'static {
    move |req: &HttpMockRequest| {
        let body: Value = serde_json::from_slice(req.body_ref()).unwrap_or(Value::Null);
        bodies.lock().expect("lock").push(body);
        json_response(status, &reply)
    }
}

pub fn json_response(status: u16, body: &Value) -> HttpMockResponse {
    HttpMockResponse::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body.to_string())
        .build()
}

/// 1 件の contents API の項目
pub fn content(name: &str, kind: &str, sha: &str) -> Value {
    json!({
        "name": name,
        "path": name,
        "sha": sha,
        "size": 3,
        "type": kind,
    })
}

/// 既定ブランチのリポジトリ（main）の基本的な mock（要約・ref・HEAD のツリー）を置く。
pub fn base_mocks(server: &MockServer, owner: &str, name: &str) {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/repos/{owner}/{name}"))
            .header("authorization", format!("Bearer {TOKEN}"));
        then.status(200).json_body(repo_json(owner, name, "main"));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/repos/{owner}/{name}/git/ref/heads/main"));
        then.status(200).json_body(ref_json(HEAD));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/repos/{owner}/{name}/git/commits/{HEAD}"));
        then.status(200).json_body(json!({"tree": {"sha": TREE}}));
    });
}
