// WASM 抽出器の実行（wasmtime、WASI preview1 のコマンド、設計書 11.1）。
//
// 権限: 抽出器に与えるのは stdin（入力のバイト列）と、stdout・stderr の受け口だけ。
// preopen（ファイル）・環境変数・引数・ネットワーク（ソケット）は与えない。時計は常に 0 を返す。
// 制限: 線形メモリは `memory_bytes` まで（ResourceLimiter）、実時間は `timeout` まで（epoch による中断）、
// stdin は `stdin_bytes` まで（実行の前に確かめる）、stdout は `stdout_bytes` まで（超えた書き込みを拒否して記録する）。
// stderr は捨てる。
//
// 失敗の分類: 時間・メモリ・stdout の超過は、記録したフラグで判定する（トラップの種類には頼らない）。

use super::{ExtractError, RuntimeLimits};
use crate::extension::decode_error;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::AsyncWrite;
use wasmtime::{Config, Engine, Linker, Module, ResourceLimiter, Store};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p1::{add_to_linker_sync, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, SinkOutputStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamError, StreamResult};
use wasmtime_wasi::{HostMonotonicClock, HostWallClock, I32Exit, WasiCtxBuilder};

/// テーブル（関数参照など）の要素数の上限。抽出器には大きな表は要らない
const TABLE_MAX_ELEMENTS: usize = 100_000;

/// 拡張の Engine を作る。実時間の中断（epoch）を有効にする
pub(crate) fn new_engine() -> wasmtime::Result<Engine> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    Engine::new(&config)
}

/// wasm のバイト列をコンパイルする（不正なモジュールはここで失敗する）
pub(crate) fn compile(engine: &Engine, wasm: &[u8]) -> wasmtime::Result<Module> {
    Module::new(engine, wasm)
}

/// 毒化されたロックも中身を取り出して使う（パニックを伝播させない）
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 1 回の実行の状態（Store のデータ）
struct Host {
    wasi: WasiP1Ctx,
    limiter: MemoryLimiter,
}

/// 線形メモリとテーブルの上限。超える要求を拒否し、メモリの拒否を記録する
struct MemoryLimiter {
    max_bytes: usize,
    denied: Arc<AtomicBool>,
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_bytes {
            self.denied.store(true, Ordering::SeqCst);
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= TABLE_MAX_ELEMENTS)
    }
}

/// stdout の受け口。上限を超える書き込みは何も書かずに拒否し、超えたことを記録する
#[derive(Clone)]
struct Stdout {
    buffer: Arc<Mutex<Vec<u8>>>,
    limit: usize,
    overflow: Arc<AtomicBool>,
}

impl Stdout {
    /// 残りの容量
    fn room(&self) -> usize {
        self.limit.saturating_sub(lock(&self.buffer).len())
    }

    /// 上限内なら追加する。超えるときは書かず、記録する
    fn push(&self, data: &[u8]) -> Result<(), ()> {
        let mut buffer = lock(&self.buffer);
        if data.len() > self.limit.saturating_sub(buffer.len()) {
            self.overflow.store(true, Ordering::SeqCst);
            return Err(());
        }
        buffer.extend_from_slice(data);
        Ok(())
    }
}

impl IsTerminal for Stdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for Stdout {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }

    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }
}

impl AsyncWrite for Stdout {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.push(buf) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(()) => Poll::Ready(Err(io::Error::other("stdout limit exceeded"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[wasmtime_wasi::async_trait]
impl Pollable for Stdout {
    async fn ready(&mut self) {}
}

impl OutputStream for Stdout {
    fn write(&mut self, bytes: bytes::Bytes) -> StreamResult<()> {
        self.push(&bytes).map_err(|()| StreamError::Closed)
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        // 容量が 0 のときに呼ばれるのは、上限を超えてまだ書くものが残っているとき（p1 は書くものがあるときだけ呼ぶ）。
        // そのため、ここで超過を記録する（書き込みが部分的に進んでいても取りこぼさない）
        match self.room() {
            0 => {
                self.overflow.store(true, Ordering::SeqCst);
                Err(StreamError::Closed)
            }
            room => Ok(room),
        }
    }
}

/// 壁時計は常に 0（実際の時刻を抽出器に渡さない）
struct ZeroWallClock;

impl HostWallClock for ZeroWallClock {
    fn resolution(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// 単調時計も常に 0
struct ZeroMonotonicClock;

impl HostMonotonicClock for ZeroMonotonicClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        0
    }
}

/// 抽出器を 1 回実行する。終了コードが 0 のときは stdout を返す。
/// 終了コードが非 0 のときは、stdout の `error` の語を失敗として返す（読めなければ `ExtractorFailed`）
pub(crate) fn run(
    engine: &Engine,
    module: &Module,
    limits: &RuntimeLimits,
    input: &[u8],
) -> Result<Vec<u8>, ExtractError> {
    if input.len() as u64 > limits.stdin_bytes {
        return Err(ExtractError::TooLarge);
    }
    let denied = Arc::new(AtomicBool::new(false));
    let overflow = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    let stdout = Stdout {
        buffer: Arc::new(Mutex::new(Vec::new())),
        limit: limits.stdout_bytes,
        overflow: overflow.clone(),
    };

    // 与えるのは stdin・stdout・stderr だけ。preopen・環境変数・引数・ネットワークは設定しない
    let wasi = WasiCtxBuilder::new()
        .stdin(MemoryInputPipe::new(input.to_vec()))
        .stdout(stdout.clone())
        .stderr(SinkOutputStream)
        .allow_ip_name_lookup(false)
        .allow_tcp(false)
        .allow_udp(false)
        .wall_clock(ZeroWallClock)
        .monotonic_clock(ZeroMonotonicClock)
        .build_p1();
    let mut store = Store::new(
        engine,
        Host {
            wasi,
            limiter: MemoryLimiter {
                max_bytes: limits.memory_bytes,
                denied: denied.clone(),
            },
        },
    );
    store.limiter(|host| &mut host.limiter);
    // 締切は「設定時の epoch + 1」。ウォッチドッグが epoch を 1 つ進めると中断される
    store.set_epoch_deadline(1);
    store.epoch_deadline_trap();

    let mut linker: Linker<Host> = Linker::new(engine);
    if add_to_linker_sync(&mut linker, |host: &mut Host| &mut host.wasi).is_err() {
        return Err(ExtractError::ExtractorFailed);
    }

    let timeout = limits.timeout;
    let outcome = std::thread::scope(|scope| {
        // ウォッチドッグ: 実時間の上限を過ぎたら epoch を進めて実行を中断させる。
        // 実行が終わると送信側が切断されるため、すぐに終わる
        let (done, waiting) = mpsc::channel::<()>();
        let flag = timed_out.clone();
        scope.spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = waiting.recv_timeout(timeout) {
                flag.store(true, Ordering::SeqCst);
                engine.increment_epoch();
            }
        });
        let outcome = start(&linker, &mut store, module);
        drop(done);
        outcome
    });

    // 上限の超過を先に判定する（トラップとして現れても、原因は超過である）
    if timed_out.load(Ordering::SeqCst) {
        return Err(ExtractError::Timeout);
    }
    if denied.load(Ordering::SeqCst) {
        return Err(ExtractError::OutOfMemory);
    }
    if overflow.load(Ordering::SeqCst) {
        return Err(ExtractError::TooLarge);
    }
    let output = lock(&stdout.buffer).clone();
    match outcome {
        Ok(()) => Ok(output),
        Err(err) => match err.downcast_ref::<I32Exit>() {
            Some(I32Exit(0)) => Ok(output),
            // 終了コードが非 0 でも、stdout に正しい失敗の JSON があれば、その種類を使う
            Some(_) => Err(match decode_error(&output) {
                Some(kind) => ExtractError::Failed(kind),
                None => ExtractError::ExtractorFailed,
            }),
            // トラップ（パニック、不正な命令など）。アプリは落とさない
            None => Err(ExtractError::ExtractorFailed),
        },
    }
}

/// `_start` を呼ぶ（WASI の命令の入口）
fn start(linker: &Linker<Host>, store: &mut Store<Host>, module: &Module) -> wasmtime::Result<()> {
    let instance = linker.instantiate(&mut *store, module)?;
    let entry = instance.get_typed_func::<(), ()>(&mut *store, "_start")?;
    entry.call(&mut *store, ())
}
