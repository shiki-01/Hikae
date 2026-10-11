// 保存メモの提案の高水準 API（設計 10.4）。起動 → 生成 → 検証を 1 関数で行う。
// 失敗は `Err`、検証で弾かれた出力は `Ok(None)`。どちらもルールベースの文をそのまま使う前提。
//
// 保存操作は LLM を待たない。このサービスは呼び出し側（app 層）が非同期に呼ぶ。

use crate::catalog::Catalog;
use crate::client::{check_endpoint, Backend, ChatClient};
use crate::custom::validate_custom_gguf;
use crate::download::{installed, Installed};
use crate::error::LlmError;
use crate::prompt::{build_messages, ChangeInput};
use crate::sidecar::{LlamaServer, LlamaServerOptions};
use crate::validate::clean_summary;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// サービスの設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// モデルの保存先（`<models_dir>/<id>/<ファイル名>`）
    pub models_dir: PathBuf,
    /// 同梱された llama-server のパス（存在しなければ環境変数・PATH の順に探す）
    pub bundled_exe: Option<PathBuf>,
    /// 生成の接続先
    pub backend: Backend,
}

/// ローカル LLM による保存メモの提案
pub struct LlmService {
    backend: Backend,
    models_dir: PathBuf,
    catalog: Catalog,
    sidecar: LlamaServer,
    chat: ChatClient,
}

impl LlmService {
    /// 環境変数と PATH を読んで作る。
    pub fn new(config: ServiceConfig) -> Result<Self, LlmError> {
        let options = LlamaServerOptions::from_process_env(config.bundled_exe.clone());
        Self::with_options(config, options)
    }

    /// サイドカーの設定を指定して作る（時刻や待ち時間をテストで差し替えるため）。
    pub fn with_options(
        config: ServiceConfig,
        options: LlamaServerOptions,
    ) -> Result<Self, LlmError> {
        let catalog = Catalog::builtin().map_err(|_| LlmError::Catalog)?;
        let backend = validated_backend(config.backend)?;
        Ok(Self {
            backend,
            models_dir: config.models_dir,
            catalog,
            sidecar: LlamaServer::new(options)?,
            chat: ChatClient::new()?,
        })
    }

    /// 現在の接続先
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// 接続先を変える。Ollama の URL は検査し、正規化した値を保存する。
    /// ループバック以外のホストは拒否しない（呼び出し側は `check_endpoint` の `external_host` で警告する）。
    pub async fn set_backend(&mut self, backend: Backend) -> Result<(), LlmError> {
        let next = validated_backend(backend)?;
        if next != self.backend {
            // 接続先が変わったら、前の接続先の内蔵サーバーは止める
            self.sidecar.stop().await;
        }
        self.backend = next;
        Ok(())
    }

    /// 同梱のモデルカタログ
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// モデルの保存先
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// モデルが導入済みかどうか（導入状態は `download::installed` と同じ判定）。
    pub fn is_model_ready(&self, model_id: &str) -> Result<bool, LlmError> {
        let entry = self.catalog.get(model_id).ok_or(LlmError::UnknownModel)?;
        Ok(installed(&self.models_dir, entry).map_err(|_| LlmError::Io)? == Installed::Ready)
    }

    /// 内蔵の llama-server を止める（アプリの終了時など）。
    pub async fn stop(&self) {
        self.sidecar.stop().await;
    }

    /// 保存メモの提案を 1 文生成する。
    ///
    /// - `model_ref`: 内蔵の接続先ではカタログの ID、Ollama ではモデル名
    /// - `timeout`: 生成（HTTP 要求）の上限。起動待ち（内蔵）は含まず、別に上限がある（`sidecar`）。
    ///   呼び出し側が全体の上限を管理し、残りの時間を渡す
    /// - 変更ファイルが空なら、サーバーを起動せず `Ok(None)` を返す
    /// - 検証で弾かれた出力は `Ok(None)`。接続・起動・タイムアウトなどは `Err`
    /// - 内蔵の生成で応答が壊れた・接続が切れたときだけ子プロセスを止める（次回に起動し直す）。
    ///   タイムアウトや呼び出しの取り消しでは止めない（止めると次回の起動が遅くなるため）
    pub async fn suggest(
        &self,
        input: &ChangeInput,
        model_ref: &str,
        timeout: Duration,
    ) -> Result<Option<String>, LlmError> {
        if input.files.is_empty() {
            return Ok(None);
        }
        if !is_valid_model_ref(model_ref) {
            return Err(LlmError::InvalidModel);
        }
        let messages = build_messages(input);
        let raw = match &self.backend {
            Backend::Builtin => {
                let entry = self.catalog.get(model_ref).ok_or(LlmError::UnknownModel)?;
                if installed(&self.models_dir, entry).map_err(|_| LlmError::Io)? != Installed::Ready
                {
                    return Err(LlmError::ModelNotInstalled);
                }
                let path = self.models_dir.join(&entry.id).join(&entry.file_name);
                let base_url = self.sidecar.ensure_started(&path).await?;
                let result = self
                    .chat
                    .chat(&base_url, &entry.file_name, &messages, timeout)
                    .await;
                if let Err(e) = &result {
                    if breaks_server(e) {
                        self.sidecar.stop().await;
                    }
                }
                result?
            }
            Backend::Ollama { base_url } => {
                let endpoint = check_endpoint(base_url)?;
                self.chat
                    .chat(&endpoint.base_url, model_ref, &messages, timeout)
                    .await?
            }
        };
        Ok(clean_summary(&raw))
    }

    /// 利用者が用意した GGUF（カスタム）で保存メモの提案を 1 文生成する（内蔵の接続先だけ）。
    ///
    /// - 先に `validate_custom_gguf` で検査する（読み取りのみ。拒否は `CustomModelInvalid`）
    /// - ファイルはコピー・移動しない。`-m` に渡すパスを内蔵のサーバーに与えるだけ
    /// - 停止・再起動の扱いは `suggest` と同じ（応答が壊れたときだけ止める）
    /// - 変更ファイルが空なら、サーバーを起動せず `Ok(None)` を返す
    pub async fn suggest_custom(
        &self,
        input: &ChangeInput,
        gguf_path: &Path,
        timeout: Duration,
    ) -> Result<Option<String>, LlmError> {
        if input.files.is_empty() {
            return Ok(None);
        }
        if self.backend != Backend::Builtin {
            return Err(LlmError::InvalidModel);
        }
        validate_custom_gguf(gguf_path).map_err(|_| LlmError::CustomModelInvalid)?;
        let messages = build_messages(input);
        let base_url = self.sidecar.ensure_started(gguf_path).await?;
        let result = self
            .chat
            .chat(&base_url, CUSTOM_MODEL_NAME, &messages, timeout)
            .await;
        if let Err(e) = &result {
            if breaks_server(e) {
                self.sidecar.stop().await;
            }
        }
        Ok(clean_summary(&result?))
    }
}

/// カスタム GGUF のとき、接続先へ渡すモデル名。パスは渡さない（ログや要求の内容に残さないため）
const CUSTOM_MODEL_NAME: &str = "custom-gguf";

/// 生成の失敗が、サーバーを止めて起動し直す理由になるか。
/// タイムアウトは含めない（サーバーは正常で、処理が長いだけのことが多いため）。
fn breaks_server(e: &LlmError) -> bool {
    matches!(
        e,
        LlmError::Network
            | LlmError::BadStatus
            | LlmError::BadResponse
            | LlmError::ResponseTooLarge
    )
}

/// 接続先を検査して正規化する。内蔵はそのまま通す。
fn validated_backend(backend: Backend) -> Result<Backend, LlmError> {
    match backend {
        Backend::Builtin => Ok(Backend::Builtin),
        Backend::Ollama { base_url } => Ok(Backend::Ollama {
            base_url: check_endpoint(&base_url)?.base_url,
        }),
    }
}

/// モデル指定の文字列として妥当か（空、制御文字、極端な長さを拒否する）。
fn is_valid_model_ref(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_broken_connections_stop_the_server() {
        // 応答が壊れた・接続が切れたときは止めて起動し直す
        for e in [
            LlmError::Network,
            LlmError::BadStatus,
            LlmError::BadResponse,
            LlmError::ResponseTooLarge,
        ] {
            assert!(breaks_server(&e), "{e:?}");
        }
        // タイムアウトや、そのほかの失敗では止めない（処理が長いだけのことが多く、起動し直すと遅くなる）
        for e in [
            LlmError::Timeout,
            LlmError::ModelNotInstalled,
            LlmError::ServerNotFound,
        ] {
            assert!(!breaks_server(&e), "{e:?}");
        }
    }
}
