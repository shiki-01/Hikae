// カスタム GGUF（利用者が用意したモデル）の検証と、偽の llama-server での生成の統合テスト。
// 実ファイル（一時ディレクトリ）を使い、拒否されたものは起動されないことを起動記録（starts.log）で確かめる。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    custom_model_id, validate_custom_gguf, Backend, ChangeInput, ChangeKind, ChangedFile,
    CustomGgufError, LlamaServerOptions, LlmError, LlmService, ServiceConfig, GENERATE_TIMEOUT,
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

/// 有効な GGUF の見た目（先頭が GGUF）のファイルを書く
fn write_gguf(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&[3, 0, 0, 0]);
    std::fs::write(&path, bytes).unwrap();
    path
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

fn service(dir: &Path, backend: Backend) -> LlmService {
    let config = ServiceConfig {
        models_dir: dir.to_path_buf(),
        bundled_exe: Some(PathBuf::from(FAKE)),
        backend,
    };
    LlmService::with_options(config, fake_options()).unwrap()
}

/// 偽サーバーが起動された回数（起動記録は、起動した `-m` のファイルと同じフォルダに残る）
fn starts(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("starts.log"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[test]
fn accepts_a_gguf_file_without_copying_or_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_gguf(dir.path(), "mine.gguf");
    let before = std::fs::read(&path).unwrap();
    let ok = validate_custom_gguf(&path).unwrap();
    assert_eq!(ok.size_bytes, 8);
    // 検証は読み取りだけ。フォルダに余計なファイルを作らない
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["mine.gguf".to_string()]);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn rejects_missing_directory_wrong_extension_and_bad_magic() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        validate_custom_gguf(&dir.path().join("none.gguf")),
        Err(CustomGgufError::NotFound)
    );
    std::fs::create_dir(dir.path().join("dir.gguf")).unwrap();
    assert_eq!(
        validate_custom_gguf(&dir.path().join("dir.gguf")),
        Err(CustomGgufError::NotAFile)
    );
    let wrong_ext = write_gguf(dir.path(), "model.safetensors");
    assert_eq!(
        validate_custom_gguf(&wrong_ext),
        Err(CustomGgufError::WrongExtension)
    );
    let bad = dir.path().join("bad.gguf");
    std::fs::write(&bad, b"ZIP!1234").unwrap();
    assert_eq!(validate_custom_gguf(&bad), Err(CustomGgufError::NotGguf));
}

#[cfg(unix)]
#[test]
fn rejects_a_symbolic_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = write_gguf(dir.path(), "real.gguf");
    let link = dir.path().join("link.gguf");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(validate_custom_gguf(&link), Err(CustomGgufError::Link));
}

#[tokio::test]
async fn invalid_custom_file_is_refused_before_starting_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.gguf");
    std::fs::write(&bad, b"NOPE").unwrap();
    let svc = service(dir.path(), Backend::Builtin);
    assert_eq!(
        svc.suggest_custom(&one_file_input(), &bad, GENERATE_TIMEOUT)
            .await,
        Err(LlmError::CustomModelInvalid)
    );
    assert_eq!(starts(dir.path()), 0);
}

#[tokio::test]
async fn empty_input_returns_none_without_starting_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_gguf(dir.path(), "mine.gguf");
    let svc = service(dir.path(), Backend::Builtin);
    let out = svc
        .suggest_custom(&ChangeInput { files: vec![] }, &path, GENERATE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(out, None);
    assert_eq!(starts(dir.path()), 0);
}

#[tokio::test]
async fn custom_gguf_is_started_with_its_own_path_and_answers() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_gguf(dir.path(), "mine.gguf");
    let svc = service(dir.path(), Backend::Builtin);
    let out = svc
        .suggest_custom(&one_file_input(), &path, GENERATE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(out.as_deref(), Some("保存メモです。"));
    // 偽サーバーは `-m` のファイルの存在を確かめ、そのフォルダに起動を記録する。
    // 記録が同じフォルダに残ること＝渡したパスで起動されたことの確認になる
    assert_eq!(starts(dir.path()), 1);
    // 同じファイルへの 2 回目の要求は、起動し直さずに使い回す
    let again = svc
        .suggest_custom(&one_file_input(), &path, GENERATE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(again.as_deref(), Some("保存メモです。"));
    assert_eq!(starts(dir.path()), 1);
    svc.stop().await;
}

#[tokio::test]
async fn switching_to_another_custom_file_restarts_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let first = write_gguf(dir.path(), "first.gguf");
    let second = write_gguf(dir.path(), "second.gguf");
    let svc = service(dir.path(), Backend::Builtin);
    svc.suggest_custom(&one_file_input(), &first, GENERATE_TIMEOUT)
        .await
        .unwrap();
    svc.suggest_custom(&one_file_input(), &second, GENERATE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(starts(dir.path()), 2);
    svc.stop().await;
}

#[tokio::test]
async fn custom_gguf_is_refused_under_the_ollama_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_gguf(dir.path(), "mine.gguf");
    let svc = service(
        dir.path(),
        Backend::Ollama {
            base_url: "http://127.0.0.1:9".to_string(),
        },
    );
    assert_eq!(
        svc.suggest_custom(&one_file_input(), &path, GENERATE_TIMEOUT)
            .await,
        Err(LlmError::InvalidModel)
    );
    assert_eq!(starts(dir.path()), 0);
}

#[test]
fn custom_model_id_uses_the_documented_form() {
    let id = custom_model_id(Path::new("D:\\gguf\\x.gguf"));
    assert!(id.starts_with("custom:"));
    assert_eq!(id.len(), 7 + 12);
}
