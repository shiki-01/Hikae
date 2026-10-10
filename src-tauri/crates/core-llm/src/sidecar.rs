// 内蔵の llama-server（サイドカー）の起動・監視・停止（設計 10.5）。
//
// - 生成が必要になった時点で起動し、最後の使用から一定時間（既定 5 分）使わなければ停止する
// - 同時に呼ばれても二重に起動しない（状態の Mutex を起動の間も保持する）
// - 待ち受けは 127.0.0.1 の空きポートのみ。子プロセスの標準入出力は捨てる（内容をログに出さない）
// - 異常終了や応答しない状態を検知したら、次の `ensure_started` で起動し直す
// - `Child` は `kill_on_drop` を付けているため、`LlamaServer` を破棄すると子プロセスも終了する

use crate::client::http_client;
use crate::error::LlmError;
use reqwest::Client;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// 実行ファイルの場所を指定する環境変数（設計 10.5 と CLAUDE.md の探索順）
pub const ENV_SERVER_PATH: &str = "HIKAE_LLAMA_SERVER_PATH";

#[cfg(windows)]
const EXE_NAME: &str = "llama-server.exe";
#[cfg(not(windows))]
const EXE_NAME: &str = "llama-server";

/// Windows でコンソール窓を出さないためのフラグ（CREATE_NO_WINDOW）
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 文脈長（設計 10.4 の入力上限に合わせる）
const CONTEXT_SIZE: u32 = 4096;
/// 並列数（単一の生成だけを扱う）
const PARALLEL_SLOTS: u32 = 1;
/// 既定のアイドル時間（設計 10.5）
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// 起動から `/health` が 200 になるまでの上限
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
/// アイドル・異常終了を見回る間隔
const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_secs(5);
/// `/health` の確認間隔
const HEALTH_INTERVAL: Duration = Duration::from_millis(200);
/// `/health` 1 回の上限
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 時刻の供給元。テストでは手動で進められるものを注入する。
pub trait Clock: Send + Sync + 'static {
    /// 基準時刻からの経過時間
    fn now(&self) -> Duration;
}

/// OS の単調時計
pub struct SystemClock {
    start: std::time::Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// スレッド数の既定値（論理コア数の半分。最小 2、最大 8）
pub fn default_threads(logical_cores: usize) -> u32 {
    ((logical_cores / 2) as u32).clamp(2, 8)
}

/// 実行ファイルを探す。順序は「環境変数（ファイルが存在する場合）→ 同梱パス → PATH」。
pub fn resolve_executable(
    env_value: Option<&OsStr>,
    bundled: Option<&Path>,
    path_value: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(p) = env_value.map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(p) = bundled.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let path_value = path_value?;
    std::env::split_paths(path_value)
        .map(|dir| dir.join(EXE_NAME))
        .find(|candidate| candidate.is_file())
}

/// サイドカーの設定
pub struct LlamaServerOptions {
    /// 環境変数 `HIKAE_LLAMA_SERVER_PATH` の値（`from_process_env` で読む）
    pub env_exe: Option<OsString>,
    /// アプリに同梱された実行ファイルのパス（呼び出し側が渡す）
    pub bundled_exe: Option<PathBuf>,
    /// PATH の値（`from_process_env` で読む）
    pub path_var: Option<OsString>,
    /// `-t` に渡すスレッド数
    pub threads: u32,
    /// この時間使われなければ停止する
    pub idle_timeout: Duration,
    /// 起動の待ち時間の上限
    pub startup_timeout: Duration,
    /// アイドルと異常終了を見回る間隔
    pub watch_interval: Duration,
    /// 時刻の供給元
    pub clock: Arc<dyn Clock>,
}

impl LlamaServerOptions {
    /// 既定値で作る。環境変数と PATH は読まない（テストで結果を固定するため）。
    pub fn new(bundled_exe: Option<PathBuf>) -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        Self {
            env_exe: None,
            bundled_exe,
            path_var: None,
            threads: default_threads(cores),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            watch_interval: DEFAULT_WATCH_INTERVAL,
            clock: Arc::new(SystemClock::new()),
        }
    }

    /// 実行プロセスの環境変数と PATH を読んで作る。
    pub fn from_process_env(bundled_exe: Option<PathBuf>) -> Self {
        let mut opts = Self::new(bundled_exe);
        opts.env_exe = std::env::var_os(ENV_SERVER_PATH);
        opts.path_var = std::env::var_os("PATH");
        opts
    }
}

struct Running {
    /// 世代番号。古い監視タスクが新しい起動を止めないために使う
    gen: u64,
    child: Child,
    model: PathBuf,
    base_url: String,
    last_used: Duration,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    next_gen: u64,
}

struct Shared {
    opts: LlamaServerOptions,
    http: Client,
    state: Mutex<State>,
}

/// 内蔵の llama-server の管理。
pub struct LlamaServer {
    shared: Arc<Shared>,
}

impl LlamaServer {
    pub fn new(opts: LlamaServerOptions) -> Result<Self, LlmError> {
        Ok(Self {
            shared: Arc::new(Shared {
                opts,
                http: http_client()?,
                state: Mutex::new(State::default()),
            }),
        })
    }

    /// 指定モデルで起動済みにし、基底 URL（`http://127.0.0.1:<port>`）を返す。
    /// 起動していれば同じ URL を返し、最終使用時刻を更新する。モデルが違えば起動し直す。
    pub async fn ensure_started(&self, model_path: &Path) -> Result<String, LlmError> {
        let shared = &self.shared;
        let mut state = shared.state.lock().await;

        if let Some(running) = state.running.as_mut() {
            let exited = matches!(running.child.try_wait(), Ok(Some(_)) | Err(_));
            let reusable = !exited
                && running.model == model_path
                && probe_health(&shared.http, &running.base_url).await;
            if reusable {
                running.last_used = shared.opts.clock.now();
                return Ok(running.base_url.clone());
            }
        }
        // 使えない古い起動は止めてから作り直す
        if let Some(mut old) = state.running.take() {
            kill(&mut old.child).await;
        }

        let exe = resolve_executable(
            shared.opts.env_exe.as_deref(),
            shared.opts.bundled_exe.as_deref(),
            shared.opts.path_var.as_deref(),
        )
        .ok_or(LlmError::ServerNotFound)?;

        let port = free_port()?;
        let base_url = format!("http://127.0.0.1:{port}");
        let mut command = Command::new(&exe);
        command
            .arg("-m")
            .arg(model_path)
            .args(["--host", "127.0.0.1"])
            .arg("--port")
            .arg(port.to_string())
            .arg("-c")
            .arg(CONTEXT_SIZE.to_string())
            .arg("-t")
            .arg(shared.opts.threads.to_string())
            .arg("--no-webui")
            .arg("--jinja")
            .arg("--parallel")
            .arg(PARALLEL_SLOTS.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command.spawn().map_err(|_| LlmError::ServerStartFailed)?;
        if let Err(e) = wait_until_healthy(
            &mut child,
            &shared.http,
            &base_url,
            shared.opts.startup_timeout,
        )
        .await
        {
            kill(&mut child).await;
            return Err(e);
        }

        state.next_gen += 1;
        let gen = state.next_gen;
        state.running = Some(Running {
            gen,
            child,
            model: model_path.to_path_buf(),
            base_url: base_url.clone(),
            last_used: shared.opts.clock.now(),
        });
        spawn_watchdog(Arc::downgrade(&self.shared), gen);
        Ok(base_url)
    }

    /// 起動している子プロセスを止める。
    pub async fn stop(&self) {
        let mut state = self.shared.state.lock().await;
        if let Some(mut running) = state.running.take() {
            kill(&mut running.child).await;
        }
    }

    /// 起動しているかどうか（子プロセスがまだ生きているかは確認しない）。
    pub async fn is_running(&self) -> bool {
        self.shared.state.lock().await.running.is_some()
    }
}

/// `/health` が 200 を返すかどうか。
async fn probe_health(http: &Client, base_url: &str) -> bool {
    match http
        .get(format!("{base_url}/health"))
        .timeout(HEALTH_PROBE_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

/// `/health` が 200 になるまで待つ。途中で子プロセスが終了した場合と、上限を過ぎた場合は失敗する。
async fn wait_until_healthy(
    child: &mut Child,
    http: &Client,
    base_url: &str,
    timeout: Duration,
) -> Result<(), LlmError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            return Err(LlmError::ServerStartFailed);
        }
        if probe_health(http, base_url).await {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(LlmError::ServerStartTimeout);
        }
        tokio::time::sleep(HEALTH_INTERVAL).await;
    }
}

async fn kill(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// 127.0.0.1 の空きポートを 1 つ選ぶ。
fn free_port() -> Result<u16, LlmError> {
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|_| LlmError::ServerStartFailed)?;
    let port = listener
        .local_addr()
        .map_err(|_| LlmError::ServerStartFailed)?
        .port();
    Ok(port)
}

/// アイドルと異常終了を見回るタスク。世代が変わるか、管理者が破棄されると終わる。
fn spawn_watchdog(weak: std::sync::Weak<Shared>, gen: u64) {
    tokio::spawn(async move {
        loop {
            let interval = match weak.upgrade() {
                Some(shared) => shared.opts.watch_interval,
                None => return,
            };
            tokio::time::sleep(interval).await;
            let Some(shared) = weak.upgrade() else {
                return;
            };
            let mut state = shared.state.lock().await;
            let stop = match state.running.as_mut() {
                Some(running) if running.gen == gen => {
                    let exited = matches!(running.child.try_wait(), Ok(Some(_)) | Err(_));
                    let idle = shared.opts.clock.now().saturating_sub(running.last_used);
                    exited || idle >= shared.opts.idle_timeout
                }
                _ => return,
            };
            if stop {
                if let Some(mut running) = state.running.take() {
                    kill(&mut running.child).await;
                }
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn default_threads_follow_half_of_cores_with_bounds() {
        assert_eq!(default_threads(0), 2);
        assert_eq!(default_threads(1), 2);
        assert_eq!(default_threads(4), 2);
        assert_eq!(default_threads(8), 4);
        assert_eq!(default_threads(16), 8);
        assert_eq!(default_threads(64), 8);
    }

    #[test]
    fn executable_resolution_order_env_then_bundled_then_path() {
        let dir = tempfile::tempdir().unwrap();
        let env_file = dir.path().join("from-env.exe");
        let bundled = dir.path().join("bundled").join(EXE_NAME);
        let path_dir = dir.path().join("pathdir");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::write(&env_file, b"").unwrap();
        std::fs::write(&bundled, b"").unwrap();
        std::fs::write(path_dir.join(EXE_NAME), b"").unwrap();
        let path_value: OsString = std::env::join_paths([path_dir.clone()]).unwrap();
        let env_os = env_file.clone().into_os_string();

        // 環境変数のファイルが存在すれば最優先
        assert_eq!(
            resolve_executable(Some(&env_os), Some(&bundled), Some(&path_value)),
            Some(env_file.clone())
        );
        // 環境変数が存在しないファイルを指す場合は次の候補へ
        let missing = dir.path().join("missing.exe").into_os_string();
        assert_eq!(
            resolve_executable(Some(&missing), Some(&bundled), Some(&path_value)),
            Some(bundled.clone())
        );
        // 同梱がなければ PATH
        assert_eq!(
            resolve_executable(None, None, Some(&path_value)),
            Some(path_dir.join(EXE_NAME))
        );
        // どこにもなければ None
        assert_eq!(resolve_executable(None, None, None), None);
        let empty_path: OsString = std::env::join_paths(Vec::<PathBuf>::new()).unwrap();
        assert_eq!(resolve_executable(None, None, Some(&empty_path)), None);
    }

    #[test]
    fn options_new_does_not_read_environment() {
        let opts = LlamaServerOptions::new(None);
        assert!(opts.env_exe.is_none());
        assert!(opts.path_var.is_none());
        assert_eq!(opts.idle_timeout, Duration::from_secs(300));
        assert!((2..=8).contains(&opts.threads));
    }
}
