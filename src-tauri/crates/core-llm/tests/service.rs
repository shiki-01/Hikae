// 保存メモ提案 API（LlmService）の統合テスト。Ollama の接続先は偽サーバーで代用し、内蔵はモデル未導入の確認に使う。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    check_endpoint, Backend, ChangeInput, ChangeKind, ChangedFile, LlamaServer, LlamaServerOptions,
    LlmError, LlmService, ServiceConfig,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-llama-server");

fn one_file_input() -> ChangeInput {
    ChangeInput {
        files: vec![ChangedFile {
            kind: ChangeKind::Modified,
            name: "報告書.docx".into(),
            old_name: None,
            amount: Some("段落 +1 / -0".into()),
            diff_text: Some("+ 実験条件を追加した".into()),
        }],
    }
}

fn config(dir: &Path, backend: Backend) -> ServiceConfig {
    ServiceConfig {
        models_dir: dir.to_path_buf(),
        bundled_exe: Some(PathBuf::from(FAKE)),
        backend,
    }
}

fn fake_options() -> LlamaServerOptions {
    LlamaServerOptions {
        env_exe: None,
        bundled_exe: Some(PathBuf::from(FAKE)),
        path_var: None,
        threads: 2,
        idle_timeout: Duration::from_secs(300),
        startup_timeout: Duration::from_secs(10),
        watch_interval: Duration::from_millis(50),
        clock: std::sync::Arc::new(core_llm::SystemClock::new()),
    }
}

#[tokio::test]
async fn empty_input_returns_none_without_starting_anything() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        LlmService::with_options(config(dir.path(), Backend::Builtin), fake_options()).unwrap();
    let out = service
        .suggest(&ChangeInput { files: vec![] }, "gemma-4-e2b-it-q4_k_m")
        .await
        .unwrap();
    assert_eq!(out, None);
}

#[tokio::test]
async fn builtin_without_installed_model_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        LlmService::with_options(config(dir.path(), Backend::Builtin), fake_options()).unwrap();
    assert_eq!(
        service
            .suggest(&one_file_input(), "gemma-4-e2b-it-q4_k_m")
            .await,
        Err(LlmError::ModelNotInstalled)
    );
    assert_eq!(
        service.suggest(&one_file_input(), "no-such-model").await,
        Err(LlmError::UnknownModel)
    );
    assert_eq!(
        service.suggest(&one_file_input(), "").await,
        Err(LlmError::InvalidModel)
    );
    assert_eq!(service.is_model_ready("gemma-4-e2b-it-q4_k_m"), Ok(false));
}

#[tokio::test]
async fn ollama_backend_returns_validated_sentence() {
    // 偽サーバーを内蔵の起動として立て、その URL を Ollama の接続先にする
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"fake").unwrap();
    let server = LlamaServer::new(fake_options()).unwrap();
    let base = server.ensure_started(&model).await.unwrap();

    let service = LlmService::with_options(
        config(
            dir.path(),
            Backend::Ollama {
                base_url: base.clone(),
            },
        ),
        fake_options(),
    )
    .unwrap();
    let out = service
        .suggest(&one_file_input(), "fake-model")
        .await
        .unwrap();
    assert_eq!(out.as_deref(), Some("保存メモです。"));

    server.stop().await;
}

#[tokio::test]
async fn ollama_connection_failure_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    // 使われていないポートを指す（接続できないことを確認する）
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let service = LlmService::with_options(
        config(
            dir.path(),
            Backend::Ollama {
                base_url: format!("http://127.0.0.1:{port}"),
            },
        ),
        fake_options(),
    )
    .unwrap();
    assert_eq!(
        service.suggest(&one_file_input(), "fake-model").await,
        Err(LlmError::Network)
    );
}

#[tokio::test]
async fn set_backend_rejects_userinfo_and_reports_external_host() {
    let dir = tempfile::tempdir().unwrap();
    let mut service =
        LlmService::with_options(config(dir.path(), Backend::Builtin), fake_options()).unwrap();
    assert_eq!(
        service
            .set_backend(Backend::Ollama {
                base_url: "http://user:pw@localhost:11434".into()
            })
            .await,
        Err(LlmError::InvalidEndpoint)
    );
    // 外部ホストは拒否せず、警告用のフラグを返す
    let external = check_endpoint("http://192.168.1.20:11434").unwrap();
    assert!(external.external_host);
    service
        .set_backend(Backend::Ollama {
            base_url: "http://192.168.1.20:11434/".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        service.backend(),
        &Backend::Ollama {
            base_url: "http://192.168.1.20:11434".into()
        }
    );
}
