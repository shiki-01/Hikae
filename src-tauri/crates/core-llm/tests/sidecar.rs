// サイドカー（llama-server の管理）の統合テスト。偽の llama-server（fake-llama-server）を実際に起動して確かめる。
// 時刻は手動で進める Clock を注入し、5 分のアイドル停止を待たずに確かめる。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{ChatClient, Clock, LlamaServer, LlamaServerOptions, LlmError, Message, Role};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Cargo がビルドした偽の llama-server のパス
const FAKE: &str = env!("CARGO_BIN_EXE_fake-llama-server");

/// 手動で進める時計（ミリ秒）
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(0)))
    }

    pub fn advance(&self, d: Duration) {
        self.0.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

fn options(clock: Arc<dyn Clock>) -> LlamaServerOptions {
    LlamaServerOptions {
        env_exe: None,
        bundled_exe: Some(PathBuf::from(FAKE)),
        path_var: None,
        threads: 2,
        idle_timeout: Duration::from_secs(5 * 60),
        startup_timeout: Duration::from_secs(10),
        watch_interval: Duration::from_millis(20),
        clock,
    }
}

fn model(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"fake model").unwrap();
    path
}

fn starts(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("starts.log"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}

async fn health_ok(base: &str) -> bool {
    match http().get(format!("{base}/health")).send().await {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// 子プロセスが止まり、接続できなくなるまで待つ（最大 10 秒）
async fn wait_until_down(base: &str) -> bool {
    for _ in 0..100 {
        if !health_ok(base).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn wait_until_stopped(server: &LlamaServer) -> bool {
    for _ in 0..100 {
        if !server.is_running().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn user_message() -> Vec<Message> {
    vec![Message {
        role: Role::User,
        content: "テスト".into(),
    }]
}

#[tokio::test]
async fn concurrent_calls_start_only_one_process() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let server = LlamaServer::new(options(ManualClock::new())).unwrap();

    let (a, b) = tokio::join!(server.ensure_started(&m), server.ensure_started(&m));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a, b);
    assert!(a.starts_with("http://127.0.0.1:"));
    assert!(health_ok(&a).await);
    assert_eq!(starts(dir.path()), 1);

    // 起動済みの間は同じ URL を返し、起動し直さない
    let c = server.ensure_started(&m).await.unwrap();
    assert_eq!(c, a);
    assert_eq!(starts(dir.path()), 1);

    server.stop().await;
    assert!(wait_until_down(&a).await);
}

#[tokio::test]
async fn idle_server_stops_five_minutes_after_last_use() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let clock = ManualClock::new();
    let server = LlamaServer::new(options(clock.clone())).unwrap();

    let base = server.ensure_started(&m).await.unwrap();
    clock.advance(Duration::from_secs(299));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server.is_running().await);
    assert!(health_ok(&base).await);

    // 使用時刻を更新すると、そこから 5 分数える
    let again = server.ensure_started(&m).await.unwrap();
    assert_eq!(again, base);
    clock.advance(Duration::from_secs(299));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server.is_running().await);

    clock.advance(Duration::from_secs(2));
    assert!(wait_until_stopped(&server).await);
    assert!(wait_until_down(&base).await);
}

#[tokio::test]
async fn dropping_server_kills_child_process() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let server = LlamaServer::new(options(ManualClock::new())).unwrap();
    let base = server.ensure_started(&m).await.unwrap();
    assert!(health_ok(&base).await);

    drop(server);
    assert!(
        wait_until_down(&base).await,
        "破棄後も子プロセスが残っている"
    );
}

#[tokio::test]
async fn restarts_after_abnormal_exit() {
    let dir = tempfile::tempdir().unwrap();
    // このファイル名の偽サーバーは chat を受けると終了する
    let m = model(dir.path(), "exit-on-chat.gguf");
    let server = LlamaServer::new(options(ManualClock::new())).unwrap();

    let first = server.ensure_started(&m).await.unwrap();
    let chat = ChatClient::new().unwrap();
    let result = chat
        .chat(&first, "x", &user_message(), Duration::from_secs(5))
        .await;
    assert!(result.is_err());
    assert!(wait_until_down(&first).await);

    // 次の呼び出しで起動し直す
    let second = server.ensure_started(&m).await.unwrap();
    assert!(health_ok(&second).await);
    assert_eq!(starts(dir.path()), 2);

    server.stop().await;
}

#[tokio::test]
async fn startup_fails_when_health_never_becomes_ok() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "no-health.gguf");
    let mut opts = options(ManualClock::new());
    opts.startup_timeout = Duration::from_secs(2);
    let server = LlamaServer::new(opts).unwrap();

    let err = server.ensure_started(&m).await.unwrap_err();
    assert_eq!(err, LlmError::ServerStartTimeout);
    assert!(!server.is_running().await);
}

#[tokio::test]
async fn missing_executable_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let mut opts = options(ManualClock::new());
    opts.bundled_exe = Some(dir.path().join("no-such-llama-server.exe"));
    let server = LlamaServer::new(opts).unwrap();

    assert_eq!(
        server.ensure_started(&m).await.unwrap_err(),
        LlmError::ServerNotFound
    );
}

#[tokio::test]
async fn chat_and_model_list_work_against_fake_server() {
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let server = LlamaServer::new(options(ManualClock::new())).unwrap();
    let base = server.ensure_started(&m).await.unwrap();

    let chat = ChatClient::new().unwrap();
    let content = chat
        .chat(&base, "model.gguf", &user_message(), Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(content, "保存メモです。");

    let models = core_llm::list_ollama_models(&base).await.unwrap();
    assert_eq!(models, vec!["fake-model".to_string()]);

    server.stop().await;
}

#[test]
fn fake_server_refuses_non_loopback_host() {
    // 本物の llama-server に外部公開させないことを、テスト側の偽サーバーでも確かめる
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "model.gguf");
    let status = Command::new(FAKE)
        .arg("-m")
        .arg(&m)
        .args(["--host", "0.0.0.0", "--port", "1"])
        .status()
        .unwrap();
    assert!(!status.success());
}

#[tokio::test]
async fn timed_out_and_cancelled_chats_keep_the_server_running() {
    // 設計 10.4: タイムアウトと取り消しでは llama-server を止めない（次の生成は同じサーバーで行う）
    let dir = tempfile::tempdir().unwrap();
    let m = model(dir.path(), "slow-chat.gguf");
    let server = LlamaServer::new(options(ManualClock::new())).unwrap();
    let base = server.ensure_started(&m).await.unwrap();
    let chat = ChatClient::new().unwrap();

    // 1. 応答が上限より遅ければタイムアウト
    let err = chat
        .chat(
            &base,
            "slow-chat.gguf",
            &user_message(),
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
    assert_eq!(err, LlmError::Timeout);
    assert!(server.is_running().await);

    // 2. 呼び出しの future を破棄する（取り消し）
    let cancelled = tokio::time::timeout(
        Duration::from_millis(200),
        chat.chat(
            &base,
            "slow-chat.gguf",
            &user_message(),
            Duration::from_secs(10),
        ),
    )
    .await;
    assert!(cancelled.is_err());

    // 3. 取り消しの後もサーバーは生きていて、起動し直していない
    assert!(server.is_running().await);
    assert!(health_ok(&base).await);
    assert_eq!(starts(dir.path()), 1);

    // 4. 次の生成は同じサーバーで成功する
    let content = chat
        .chat(
            &base,
            "slow-chat.gguf",
            &user_message(),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(content, "保存メモです。");
    assert_eq!(starts(dir.path()), 1);

    server.stop().await;
}
