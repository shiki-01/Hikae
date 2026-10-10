// モデルのダウンロード（設計 10.5）。再開対応（`.part` への追記）、完了後の SHA-256 検証、atomic な rename。
//
// - 保存先は `<models_dir>/<id>/<ファイル名>` だけ。id・ファイル名は安全な 1 要素の名前に限る
// - 取得元は https の `huggingface.co`、`*.huggingface.co`、`*.hf.co` に限る。リダイレクトは手動で追跡し、
//   各ホップで同じ検査をする（最大 5 回）
// - 応答サイズがカタログの想定と 1% を超えて違う場合は、本文を書く前に拒否する
// - ネットワーク断は再試行しない。`.part` は残るので、呼び出し側が再実行すると続きから取得する
// - トークン・認証情報は扱わない。認証が必要な URL は想定外

use crate::catalog::CatalogEntry;
use reqwest::header::{CONTENT_RANGE, LOCATION, RANGE};
use reqwest::{redirect, Client, Url};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// リダイレクトの上限
const MAX_REDIRECTS: usize = 5;
/// カタログのサイズとの許容差（百分率）
const SIZE_TOLERANCE_PERCENT: u64 = 1;
/// 未完了ファイルの拡張子
const PART_SUFFIX: &str = ".part";
/// 名前（ID・ファイル名）の最大長
const MAX_NAME_LEN: usize = 128;

/// ダウンロードのエラー。文言は固定で、URL・パス・内容を含めない。
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadError {
    #[error("モデルの ID またはファイル名が正しくありません")]
    InvalidName,

    #[error("ダウンロード元が許可されていません")]
    DisallowedHost,

    #[error("リダイレクトが多すぎます")]
    TooManyRedirects,

    #[error("ダウンロードするファイルのサイズが想定と一致しません")]
    SizeMismatch,

    #[error("ダウンロードしたファイルの検証に失敗しました。ファイルを削除しました")]
    HashMismatch,

    #[error("サーバーがエラーを返しました（状態コード {0}）")]
    HttpStatus(u16),

    #[error("ネットワークエラーが発生しました。もう一度実行すると続きから再開します")]
    Network,

    #[error("ダウンロードを中止しました。もう一度実行すると続きから再開します")]
    Canceled,

    #[error("サーバーの応答が正しくありません")]
    BadResponse,

    #[error("ファイル操作に失敗しました")]
    Io,
}

/// 導入状態。`Ready` は最終ファイルがあり、サイズがカタログと一致することを示す（SHA-256 は導入時に検証済み）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    Missing,
    Partial { bytes: u64 },
    Ready,
}

/// 取得元の検査規則。本番は `Production` 固定。テスト専用の規則はテストビルドにだけ存在する。
#[derive(Debug, Clone, Copy)]
enum HostRule {
    Production,
    #[cfg(test)]
    TestLoopback,
}

/// URL が取得元として許可されるかを検査する（本番規則）。
/// https のみ、ユーザー情報なし、ホストは `huggingface.co`、`*.huggingface.co`、`*.hf.co` のいずれか。
pub fn is_allowed_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.host_str() {
        Some(host) => {
            host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || host.ends_with(".hf.co")
        }
        None => false,
    }
}

fn is_allowed_by(rule: HostRule, url: &Url) -> bool {
    match rule {
        HostRule::Production => is_allowed_url(url),
        #[cfg(test)]
        HostRule::TestLoopback => url.scheme() == "http" && url.host_str() == Some("127.0.0.1"),
    }
}

/// ID・ファイル名として安全な 1 要素の名前か。ASCII の英数字と `-` `_` `.` のみ、先頭の `.` と `..` を禁止する。
/// パス区切り・ドライブ指定（`:`）・空白を含む名前は拒否する。
pub fn is_safe_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// モデルの保存先（ディレクトリ、最終ファイル、`.part`）。名前が安全でなければ拒否する。
struct ModelPaths {
    dir: PathBuf,
    final_file: PathBuf,
    part_file: PathBuf,
}

fn model_paths(models_dir: &Path, entry: &CatalogEntry) -> Result<ModelPaths, DownloadError> {
    if !is_safe_component(&entry.id) || !is_safe_component(&entry.file_name) {
        return Err(DownloadError::InvalidName);
    }
    let dir = models_dir.join(&entry.id);
    let final_file = dir.join(&entry.file_name);
    let part_file = dir.join(format!("{}{PART_SUFFIX}", entry.file_name));
    Ok(ModelPaths {
        dir,
        final_file,
        part_file,
    })
}

/// 導入状態を調べる（ネットワークには接続しない）。
pub fn installed(models_dir: &Path, entry: &CatalogEntry) -> Result<Installed, DownloadError> {
    let paths = model_paths(models_dir, entry)?;
    if let Some(len) = std_file_len(&paths.final_file) {
        if len == entry.size_bytes {
            return Ok(Installed::Ready);
        }
    }
    match std_file_len(&paths.part_file) {
        Some(bytes) => Ok(Installed::Partial { bytes }),
        None => Ok(Installed::Missing),
    }
}

/// 導入済みのファイルと未完了ファイルを削除する。空になったモデルのフォルダも削除する。
pub fn remove(models_dir: &Path, entry: &CatalogEntry) -> Result<(), DownloadError> {
    let paths = model_paths(models_dir, entry)?;
    for file in [&paths.final_file, &paths.part_file] {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(DownloadError::Io),
        }
    }
    // フォルダに他のファイルがあれば残す
    let _ = std::fs::remove_dir(&paths.dir);
    Ok(())
}

fn std_file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

async fn file_len(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .await
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// ファイルの SHA-256 を小文字の 16 進で返す。ブロッキング処理は専用スレッドで行う。
async fn sha256_of_file(path: PathBuf) -> Result<String, DownloadError> {
    let digest = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(to_hex(&hasher.finalize()))
    })
    .await
    .map_err(|_| DownloadError::Io)?;
    digest.map_err(|_| DownloadError::Io)
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// モデルのダウンロード。HTTP クライアントは 1 つ作って使い回す。
pub struct Downloader {
    client: Client,
    models_dir: PathBuf,
    rule: HostRule,
}

impl Downloader {
    /// 本番用。取得元は `is_allowed_url` の規則に限る。
    pub fn new(models_dir: impl Into<PathBuf>) -> Result<Self, DownloadError> {
        Self::build(models_dir.into(), HostRule::Production)
    }

    #[cfg(test)]
    fn for_test(models_dir: impl Into<PathBuf>) -> Result<Self, DownloadError> {
        Self::build(models_dir.into(), HostRule::TestLoopback)
    }

    fn build(models_dir: PathBuf, rule: HostRule) -> Result<Self, DownloadError> {
        // 接続とサイズ全体の上限は設けない（数 GB の取得のため）。応答が途絶えた場合だけ読み取りの上限で止める
        let builder = Client::builder()
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60));
        // テストの loopback 通信は、環境変数のプロキシを経由させない
        #[cfg(test)]
        let builder = if matches!(rule, HostRule::TestLoopback) {
            builder.no_proxy()
        } else {
            builder
        };
        let client = builder.build().map_err(|_| DownloadError::Network)?;
        Ok(Self {
            client,
            models_dir,
            rule,
        })
    }

    /// モデルを取得して検証し、最終ファイルのパスを返す。
    ///
    /// - 最終ファイルが既にあり、サイズと SHA-256 が一致すれば何もせず返す
    /// - `.part` があれば続きから取得する（応答が 206 でない場合は先頭から取り直す）
    /// - `on_progress(受信済みバイト, 総バイト)` を受信のたびに呼ぶ
    /// - キャンセルされた場合は `.part` を残して `Canceled` を返す
    pub async fn download(
        &self,
        entry: &CatalogEntry,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(u64, u64),
    ) -> Result<PathBuf, DownloadError> {
        let paths = model_paths(&self.models_dir, entry)?;
        let url = Url::parse(&entry.url).map_err(|_| DownloadError::BadResponse)?;
        if !is_allowed_by(self.rule, &url) {
            return Err(DownloadError::DisallowedHost);
        }
        let size = entry.size_bytes;

        fs::create_dir_all(&paths.dir)
            .await
            .map_err(|_| DownloadError::Io)?;

        // 既存の最終ファイル。一致すれば何もしない。一致しなければ壊れているとみなして削除する
        if file_len(&paths.final_file).await == Some(size) {
            if sha256_of_file(paths.final_file.clone()).await? == entry.sha256 {
                return Ok(paths.final_file);
            }
            remove_if_exists(&paths.final_file).await?;
        }
        if cancel.is_cancelled() {
            return Err(DownloadError::Canceled);
        }

        // 途中まで取得済みの `.part`。想定より大きいものは壊れているとみなして取り直す
        let mut offset = file_len(&paths.part_file).await.unwrap_or(0);
        if offset > size {
            remove_if_exists(&paths.part_file).await?;
            offset = 0;
        }

        if offset < size {
            self.fetch(
                url,
                offset,
                size,
                &paths.part_file,
                cancel,
                &mut on_progress,
            )
            .await?;
        }

        // 取得したファイルを検証する
        let got = file_len(&paths.part_file).await.unwrap_or(0);
        if got != size {
            return Err(DownloadError::Network);
        }
        if sha256_of_file(paths.part_file.clone()).await? != entry.sha256 {
            remove_if_exists(&paths.part_file).await?;
            remove_if_exists(&paths.final_file).await?;
            return Err(DownloadError::HashMismatch);
        }
        fs::rename(&paths.part_file, &paths.final_file)
            .await
            .map_err(|_| DownloadError::Io)?;
        Ok(paths.final_file)
    }

    /// `offset` から `size` まで取得し、`part` に追記する（応答が 200 なら先頭から書き直す）。
    async fn fetch(
        &self,
        url: Url,
        offset: u64,
        size: u64,
        part: &Path,
        cancel: &CancellationToken,
        on_progress: &mut impl FnMut(u64, u64),
    ) -> Result<(), DownloadError> {
        let mut current = url;
        let mut hops = 0usize;
        let mut resp = loop {
            if !is_allowed_by(self.rule, &current) {
                return Err(DownloadError::DisallowedHost);
            }
            let mut request = self.client.get(current.clone());
            if offset > 0 {
                request = request.header(RANGE, format!("bytes={offset}-"));
            }
            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DownloadError::Canceled),
                r = request.send() => r.map_err(|_| DownloadError::Network)?,
            };
            if response.status().is_redirection() {
                hops += 1;
                if hops > MAX_REDIRECTS {
                    return Err(DownloadError::TooManyRedirects);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(DownloadError::BadResponse)?;
                current = current
                    .join(location)
                    .map_err(|_| DownloadError::BadResponse)?;
                continue;
            }
            break response;
        };

        // 開始位置と総サイズを応答ヘッダから決める（本文はまだ読まない）
        let status = resp.status();
        let (start, total) = match status.as_u16() {
            206 => {
                let (start, total) = parse_content_range(
                    resp.headers()
                        .get(CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .ok_or(DownloadError::BadResponse)?,
                )
                .ok_or(DownloadError::BadResponse)?;
                if start != offset {
                    return Err(DownloadError::BadResponse);
                }
                (start, total)
            }
            200 => {
                let total = resp.content_length().ok_or(DownloadError::BadResponse)?;
                (0, total)
            }
            416 => {
                // 取得済みの範囲が無効。次回は先頭から取り直す
                remove_if_exists(part).await?;
                return Err(DownloadError::HttpStatus(416));
            }
            other => return Err(DownloadError::HttpStatus(other)),
        };
        if total.abs_diff(size) > size / 100 * SIZE_TOLERANCE_PERCENT {
            return Err(DownloadError::SizeMismatch);
        }

        let mut file = if start == 0 {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(part)
                .await
        } else {
            OpenOptions::new().append(true).open(part).await
        }
        .map_err(|_| DownloadError::Io)?;

        let mut received = start;
        on_progress(received, size);
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DownloadError::Canceled),
                c = resp.chunk() => c.map_err(|_| DownloadError::Network)?,
            };
            let Some(bytes) = chunk else { break };
            if received + bytes.len() as u64 > size {
                return Err(DownloadError::SizeMismatch);
            }
            file.write_all(&bytes)
                .await
                .map_err(|_| DownloadError::Io)?;
            received += bytes.len() as u64;
            on_progress(received, size);
        }
        file.sync_all().await.map_err(|_| DownloadError::Io)?;
        Ok(())
    }
}

async fn remove_if_exists(path: &Path) -> Result<(), DownloadError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DownloadError::Io),
    }
}

/// `bytes S-E/T` 形式の Content-Range から開始位置と総サイズを読む。
fn parse_content_range(value: &str) -> Option<(u64, u64)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let total = total.trim().parse::<u64>().ok()?;
    Some((start, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// テスト用の簡易 HTTP サーバーの記録。受け取った Range ヘッダを残す。
    #[derive(Default)]
    struct Log {
        ranges: Mutex<Vec<Option<u64>>>,
    }

    /// 127.0.0.1 で待ち受けるテストサーバーを起動する。
    /// - `/file.gguf`: 本体を返す（Range に対応）
    /// - `/redirect`: `/file.gguf` へ相対 URL でリダイレクト
    /// - `/evil`: 許可されていないホストへリダイレクト
    /// - `/slow`: ヘッダだけ返して本文を返さない
    async fn spawn_server(body: Arc<Vec<u8>>, log: Arc<Log>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = match sock.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        head.extend_from_slice(&buf[..n]);
                        if head.len() > 16 * 1024 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&head).to_string();
                    let mut lines = text.split("\r\n");
                    let request_line = lines.next().unwrap_or_default();
                    let path = request_line.split(' ').nth(1).unwrap_or("/").to_string();
                    let range = lines
                        .filter_map(|l| l.split_once(':'))
                        .find(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
                        .and_then(|(_, v)| {
                            v.trim()
                                .strip_prefix("bytes=")?
                                .split('-')
                                .next()?
                                .parse::<u64>()
                                .ok()
                        });
                    log.ranges.lock().unwrap().push(range);

                    let len = body.len() as u64;
                    let response: Vec<u8> = match path.as_str() {
                        "/redirect" => {
                            b"HTTP/1.1 302 Found\r\nLocation: /file.gguf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                .to_vec()
                        }
                        "/evil" => {
                            b"HTTP/1.1 302 Found\r\nLocation: http://example.com/x.gguf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                .to_vec()
                        }
                        "/slow" => {
                            let head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
                            );
                            let _ = sock.write_all(head.as_bytes()).await;
                            // 本文を送らず待つ（キャンセルの確認用）
                            tokio::time::sleep(Duration::from_secs(60)).await;
                            return;
                        }
                        _ => {
                            let (status, start) = match range {
                                Some(s) if s < len => ("206 Partial Content", s),
                                _ => ("200 OK", 0),
                            };
                            let mut h = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", len - start);
                            if start > 0 {
                                h.push_str(&format!(
                                    "Content-Range: bytes {start}-{}/{len}\r\n",
                                    len - 1
                                ));
                            }
                            h.push_str("Connection: close\r\n\r\n");
                            let mut out = h.into_bytes();
                            out.extend_from_slice(&body[start as usize..]);
                            out
                        }
                    };
                    let _ = sock.write_all(&response).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        addr
    }

    fn sha_hex(bytes: &[u8]) -> String {
        to_hex(&Sha256::digest(bytes))
    }

    fn entry_for(addr: SocketAddr, path: &str, body: &[u8], sha: &str) -> CatalogEntry {
        CatalogEntry {
            id: "test-model".into(),
            display_name: "test".into(),
            stage: 2,
            file_name: "model.gguf".into(),
            url: format!("http://{addr}{path}"),
            sha256: sha.into(),
            size_bytes: body.len() as u64,
            license_name: "Apache-2.0".into(),
            license_url: "https://www.apache.org/licenses/LICENSE-2.0".into(),
            min_total_memory_bytes: 0,
            recommended: false,
        }
    }

    fn test_body() -> Vec<u8> {
        (0..100_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn production_rule_accepts_only_hugging_face_https() {
        let ok = [
            "https://huggingface.co/unsloth/x/resolve/main/a.gguf",
            "https://us.aws.cdn.hf.co/xet-bridge-us/a?x=1",
            "https://cdn-lfs.huggingface.co/a",
        ];
        for u in ok {
            assert!(is_allowed_url(&Url::parse(u).unwrap()), "{u}");
        }
        let ng = [
            "http://huggingface.co/a",
            "https://evil.com/a",
            "https://huggingface.co.evil.com/a",
            "https://evilhf.co/a",
            "https://hf.co/a",
            "https://user:pw@huggingface.co/a",
            "https://user@huggingface.co/a",
            "http://127.0.0.1:8080/a",
            "file:///C:/a",
        ];
        for u in ng {
            assert!(!is_allowed_url(&Url::parse(u).unwrap()), "{u}");
        }
    }

    #[test]
    fn rejects_unsafe_names() {
        for n in [
            "../x", "a/b", "a\\b", "C:evil", "a b", "", ".hidden", "a..b", "x:y",
        ] {
            assert!(!is_safe_component(n), "{n}");
        }
        for n in [
            "gemma-4-e2b-it-q4_k_m",
            "model.gguf",
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        ] {
            assert!(is_safe_component(n), "{n}");
        }
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range("bytes 40000-99999/100000"),
            Some((40000, 100000))
        );
        assert_eq!(parse_content_range("bytes */100"), None);
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }

    #[test]
    fn installed_states_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let body = test_body();
        let entry = entry_for("127.0.0.1:1".parse().unwrap(), "/x", &body, &sha_hex(&body));
        assert_eq!(installed(dir.path(), &entry).unwrap(), Installed::Missing);

        let model_dir = dir.path().join(&entry.id);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.gguf.part"), &body[..500]).unwrap();
        assert_eq!(
            installed(dir.path(), &entry).unwrap(),
            Installed::Partial { bytes: 500 }
        );

        std::fs::write(model_dir.join("model.gguf"), &body).unwrap();
        assert_eq!(installed(dir.path(), &entry).unwrap(), Installed::Ready);

        remove(dir.path(), &entry).unwrap();
        assert_eq!(installed(dir.path(), &entry).unwrap(), Installed::Missing);
        assert!(!model_dir.exists());
    }

    #[test]
    fn rejects_traversal_id_before_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let body = test_body();
        let mut entry = entry_for("127.0.0.1:1".parse().unwrap(), "/x", &body, &sha_hex(&body));
        entry.id = "../escape".into();
        assert_eq!(
            installed(dir.path(), &entry),
            Err(DownloadError::InvalidName)
        );
        assert_eq!(remove(dir.path(), &entry), Err(DownloadError::InvalidName));
    }

    #[tokio::test]
    async fn fresh_download_is_verified_and_renamed() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(addr, "/file.gguf", &body, &sha_hex(&body));
        let dl = Downloader::for_test(dir.path()).unwrap();

        let mut last = (0, 0);
        let path = dl
            .download(&entry, &CancellationToken::new(), |r, t| last = (r, t))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *body);
        assert_eq!(last, (body.len() as u64, body.len() as u64));
        assert!(!path.with_file_name("model.gguf.part").exists());
        assert_eq!(log.ranges.lock().unwrap().as_slice(), &[None]);
    }

    #[tokio::test]
    async fn resumes_from_partial_file() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(addr, "/file.gguf", &body, &sha_hex(&body));
        let model_dir = dir.path().join(&entry.id);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.gguf.part"), &body[..40_000]).unwrap();

        let dl = Downloader::for_test(dir.path()).unwrap();
        let path = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *body);
        assert_eq!(log.ranges.lock().unwrap().as_slice(), &[Some(40_000)]);
    }

    #[tokio::test]
    async fn hash_mismatch_removes_files() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log).await;
        let dir = tempfile::tempdir().unwrap();
        let wrong = "0".repeat(64);
        let entry = entry_for(addr, "/file.gguf", &body, &wrong);
        let dl = Downloader::for_test(dir.path()).unwrap();

        let err = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err, DownloadError::HashMismatch);
        assert_eq!(installed(dir.path(), &entry).unwrap(), Installed::Missing);
    }

    #[tokio::test]
    async fn size_mismatch_is_rejected_before_writing() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log).await;
        let dir = tempfile::tempdir().unwrap();
        let mut entry = entry_for(addr, "/file.gguf", &body, &sha_hex(&body));
        entry.size_bytes = body.len() as u64 * 2;
        let dl = Downloader::for_test(dir.path()).unwrap();

        let err = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err, DownloadError::SizeMismatch);
        assert_eq!(installed(dir.path(), &entry).unwrap(), Installed::Missing);
    }

    #[tokio::test]
    async fn follows_relative_redirect() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log).await;
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(addr, "/redirect", &body, &sha_hex(&body));
        let dl = Downloader::for_test(dir.path()).unwrap();
        let path = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *body);
    }

    #[tokio::test]
    async fn redirect_to_other_host_is_refused() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log).await;
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(addr, "/evil", &body, &sha_hex(&body));
        let dl = Downloader::for_test(dir.path()).unwrap();
        let err = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err, DownloadError::DisallowedHost);
    }

    #[tokio::test]
    async fn production_rejects_loopback_url_without_network() {
        let body = test_body();
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for("127.0.0.1:1".parse().unwrap(), "/x", &body, &sha_hex(&body));
        let dl = Downloader::new(dir.path()).unwrap();
        let err = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err, DownloadError::DisallowedHost);
    }

    #[tokio::test]
    async fn existing_verified_file_needs_no_network() {
        let body = test_body();
        let dir = tempfile::tempdir().unwrap();
        // 接続できないアドレスを指す。通信が起きればエラーになる
        let entry = entry_for("127.0.0.1:1".parse().unwrap(), "/x", &body, &sha_hex(&body));
        let model_dir = dir.path().join(&entry.id);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.gguf"), &body).unwrap();
        let dl = Downloader::for_test(dir.path()).unwrap();
        let path = dl
            .download(&entry, &CancellationToken::new(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(path, model_dir.join("model.gguf"));
    }

    #[tokio::test]
    async fn cancel_keeps_partial_and_returns_canceled() {
        let body = Arc::new(test_body());
        let log = Arc::new(Log::default());
        let addr = spawn_server(body.clone(), log).await;
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(addr, "/slow", &body, &sha_hex(&body));
        let dl = Downloader::for_test(dir.path()).unwrap();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            dl.download(&entry, &cancel, |_, _| {}),
        )
        .await
        .expect("キャンセル後すぐに戻ること");
        assert_eq!(result.unwrap_err(), DownloadError::Canceled);
        // 再開のため .part は残す
        let model_dir = dir.path().join(&entry.id);
        assert!(model_dir.join("model.gguf.part").exists());
        assert!(!model_dir.join("model.gguf").exists());
    }
}
