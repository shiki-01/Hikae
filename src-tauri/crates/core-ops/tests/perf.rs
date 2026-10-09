// 性能の計測（設計書 1.4 の非機能要件）。通常の `cargo test` では動かない（`#[ignore]`）。
//
// 実行（1 つずつ。メモリとディスクを多く使うため `-j 2` と `--test-threads=1` を付ける）:
//
//   cd src-tauri
//   cargo test -j 2 -p core-ops --test perf -- --ignored --nocapture --test-threads=1 perf_many_files
//   cargo test -j 2 -p core-ops --test perf -- --ignored --nocapture --test-threads=1 perf_large_total
//
// 計測値は `PERF|<場面>|<項目>|<値>` の行として標準出力に出すだけで、時間のしきい値は assert しない
// （環境で揺れるため）。assert するのは、件数など結果の正しさだけ。
//
// 環境変数（任意）:
//   HIKAE_PERF_FILES        perf_many_files のファイル数（既定 10000）
//   HIKAE_PERF_HISTORY      perf_many_files で履歴を作る保存の件数（既定 130）
//   HIKAE_PERF_LARGE_FILES  perf_large_total のファイル数（既定 1000）
//   HIKAE_PERF_LARGE_MB     perf_large_total の 1 ファイルの大きさ（MB。既定 5）
//
// 作るもの: 一時ディレクトリの実リポジトリ（深さ 3〜4、日本語名を含む）。終了時に削除する。
// git は `GitRunner` 経由（同梱の git があればそれを使う）。次の 2 つだけ、計測の準備として git を
// 直接呼ぶ: (1) 保存以外の「インデックスを書く」操作の再現（`GIT_OPTIONAL_LOCKS` を付けない
// `status`）、(2) fsmonitor のデーモンの停止（一時ディレクトリを消せるようにするため）。

mod common;

use core_git::GitRunner;
use core_ops::{AutoSnapshotOutcome, Identity, Ops, SaveOutcome, SizeLimits};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 2 つの計測を同時に走らせない（結果が互いに歪むため）
static SERIAL: Mutex<()> = Mutex::new(());

/// 同梱の git の置き場（`app/resources`）
fn resource_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/resources")
}

/// 計測に使う GitRunner。同梱の git があれば優先する（アプリと同じ探索）。待ち時間は計測のため 1 時間にする
/// （既定の 60 秒を超える所要時間は、各項目の値と 60 秒を見比べて判断する）。
fn runner() -> GitRunner {
    let r = if resource_dir().join("git").exists() {
        GitRunner::bundled(&resource_dir())
    } else {
        GitRunner::from_path_env()
    };
    r.with_timeout(Duration::from_secs(3600))
}

fn ops() -> Ops {
    Ops::new(runner())
}

/// 計測の準備で直接呼ぶ git の実行ファイル（`runner()` と同じものを指す）
fn git_exe() -> PathBuf {
    if let Some(p) = std::env::var_os("HIKAE_GIT_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return p;
        }
    }
    let bundled = if cfg!(windows) {
        resource_dir().join("git/cmd/git.exe")
    } else {
        resource_dir().join("git/bin/git")
    };
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("git")
    }
}

/// git を直接呼ぶ（計測の準備用）。ユーザーの設定は読まない。`GIT_OPTIONAL_LOCKS` は付けない
fn raw_git(repo: &Path, args: &[&str]) -> (i32, Vec<u8>, String) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new(git_exe())
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .env_remove("GIT_OPTIONAL_LOCKS")
        .output()
        .expect("git を起動できません");
    (
        out.status.code().unwrap_or(-1),
        out.stdout,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// 終了時に fsmonitor のデーモンを止める（動いたままだと一時ディレクトリを消せない）
struct DaemonGuard(PathBuf);

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = raw_git(&self.0, &["fsmonitor--daemon", "stop"]);
    }
}

// ---------- 計測の出力 ----------

fn emit(scenario: &str, metric: &str, value: impl std::fmt::Display) {
    println!("PERF|{scenario}|{metric}|{value}");
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 時間を測って結果を返す
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let v = f();
    (v, start.elapsed())
}

/// 複数回の計測の最小・中央値・最大を出す
fn emit_stats(scenario: &str, metric: &str, samples: &mut [f64]) {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = samples.first().copied().unwrap_or(0.0);
    let max = samples.last().copied().unwrap_or(0.0);
    let med = samples.get(samples.len() / 2).copied().unwrap_or(0.0);
    emit(
        scenario,
        metric,
        format!(
            "min={min:.0}ms median={med:.0}ms max={max:.0}ms n={}",
            samples.len()
        ),
    );
}

// ---------- 作業フォルダの生成 ----------

/// 場面ごとのファイル構成
#[derive(Clone, Copy)]
struct Layout {
    files: usize,
    /// 1 ファイルの大きさ（バイト）を返す。0 は小さなテキスト
    big_bytes: Option<usize>,
}

/// i 番目のファイルの相対パス。深さは 3（フォルダ 2 + ファイル）か 4。5 件に 1 件は日本語名
fn rel_path(i: usize) -> String {
    let a = i % 10;
    let b = (i / 10) % 10;
    let c = (i / 100) % 10;
    let jp = i.is_multiple_of(5);
    let (top, mid, name) = if jp {
        (
            format!("資料{a}"),
            format!("第{b}章"),
            format!("報告書_{i}.txt"),
        )
    } else {
        (
            format!("docs{a}"),
            format!("part{b}"),
            format!("file_{i}.dat"),
        )
    };
    if i % 2 == 1 {
        format!("{top}/{mid}/sec{c}/{name}")
    } else {
        format!("{top}/{mid}/{name}")
    }
}

/// 圧縮しにくい擬似乱数で埋める（画像や動画のような中身を想定）
fn fill_random(buf: &mut [u8], seed: u64) {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for chunk in buf.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let bytes = x.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

/// i 番目のファイルの内容。`generation` を変えると内容が変わる
fn content(layout: Layout, i: usize, generation: u32) -> Vec<u8> {
    if let Some(bytes) = layout.big_bytes {
        let mut buf = vec![0u8; bytes];
        fill_random(&mut buf, ((i as u64) << 8) | u64::from(generation));
        return buf;
    }
    // 500 件に 1 件は 1MB の乱数、それ以外は 2KB ほどのテキスト
    if i.is_multiple_of(500) {
        let mut buf = vec![0u8; 1024 * 1024];
        fill_random(&mut buf, ((i as u64) << 8) | u64::from(generation));
        return buf;
    }
    let mut text = String::new();
    let mut n = 0;
    while text.len() < 2000 {
        text.push_str(&format!("行 {n} / ファイル {i} / 版 {generation}\n"));
        n += 1;
    }
    text.into_bytes()
}

fn write_file(repo: &Path, layout: Layout, i: usize, generation: u32) -> TestResult {
    let path = repo.join(rel_path(i));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content(layout, i, generation))?;
    Ok(())
}

fn create_files(repo: &Path, layout: Layout) -> TestResult {
    for i in 0..layout.files {
        write_file(repo, layout, i, 0)?;
    }
    Ok(())
}

/// 等間隔に選んだ 100 ファイルを書き換える（大きさは同じ。中身を変える）
fn modify_100(repo: &Path, layout: Layout, generation: u32) -> TestResult {
    let step = (layout.files / 100).max(1);
    for k in 0..100 {
        write_file(repo, layout, k * step, generation)?;
    }
    Ok(())
}

// ---------- 計測の本体 ----------

fn identity() -> Identity {
    Identity {
        name: "perf".to_string(),
        email: "perf@users.noreply.github.com".to_string(),
    }
}

const STATUS_RUNS: usize = 5;

/// 変更の一覧（`list_changes`。アプリと同じく `GIT_OPTIONAL_LOCKS=0`）と、同じ `status` を
/// ロックを許す形（`GIT_OPTIONAL_LOCKS` なし）で呼んだ場合を、それぞれ繰り返し測る。
/// `expected` は変更の件数（結果の正しさだけを assert する）。
fn measure_status(scenario: &str, variant: &str, state: &str, expected: usize) -> TestResult {
    let repo = repo_of(scenario);
    let mut app_side = Vec::new();
    for run in 0..STATUS_RUNS {
        let (r, d) = timed(|| ops().list_changes(&repo));
        assert_eq!(r?.len(), expected);
        if run == 0 {
            emit(
                scenario,
                &format!("{variant}/{state}/list_changes 1回目"),
                format!("{:.0}ms", ms(d)),
            );
        }
        app_side.push(ms(d));
    }
    emit_stats(
        scenario,
        &format!("{variant}/{state}/list_changes"),
        &mut app_side,
    );

    let mut lock_side = Vec::new();
    for _ in 0..STATUS_RUNS {
        let (r, d) = timed(|| raw_git(&repo, &["status", "--porcelain=v2", "-z", "-uall"]));
        assert_eq!(r.0, 0, "{}", r.2);
        let records = r.1.split(|b| *b == 0).filter(|s| !s.is_empty()).count();
        assert_eq!(records, expected);
        lock_side.push(ms(d));
    }
    emit_stats(
        scenario,
        &format!("{variant}/{state}/status(ロックを許す)"),
        &mut lock_side,
    );
    Ok(())
}

/// 場面ごとの作業フォルダの場所を覚えておく（`measure_status` から参照）
static REPOS: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());

fn repo_of(scenario: &str) -> PathBuf {
    let guard = REPOS.lock().expect("lock");
    guard
        .iter()
        .find(|(s, _)| s == scenario)
        .map(|(_, p)| p.clone())
        .expect("場面が登録されていません")
}

/// 変更の無い状態で、保存と同じ「インデックスを書く」操作を再現する（untracked cache や
/// fsmonitor の記録を作るため、`GIT_OPTIONAL_LOCKS` を付けない `status` を 2 回）
fn prime_index(repo: &Path) {
    for _ in 0..2 {
        let _ = raw_git(repo, &["status", "--porcelain=v2", "-z", "-uall"]);
    }
}

fn run_scenario(scenario: &str, layout: Layout, history_commits: usize) -> TestResult {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    REPOS
        .lock()
        .map_err(|e| e.to_string())?
        .push((scenario.to_string(), repo.clone()));
    let _guard = DaemonGuard(repo.clone());

    emit(scenario, "git", {
        let (_, out, _) = raw_git(tmp.path(), &["--version"]);
        format!(
            "{} / 入手元={:?}",
            String::from_utf8_lossy(&out).trim(),
            runner().source()
        )
    });
    emit(scenario, "ファイル数", layout.files);

    // 準備: ファイルの生成（計測の対象外だが、参考に時間を出す）
    ops().init_project(&repo, None, &identity())?;
    let (r, d) = timed(|| create_files(&repo, layout));
    r?;
    emit(
        scenario,
        "参考/ファイル生成",
        format!("{:.1}s", d.as_secs_f64()),
    );

    // (a) 初回保存
    let (r, d) = timed(|| ops().save(&repo, "初回の保存"));
    match r? {
        SaveOutcome::Saved { .. } => {}
        other => panic!("初回の保存が完了しませんでした: {other:?}"),
    }
    emit(scenario, "(a) 初回保存", format!("{:.1}s", d.as_secs_f64()));
    if let Ok(size) = ops().repo_size(&repo) {
        emit(
            scenario,
            "参考/保存データの大きさ(count-objects)",
            format!("{:.0}MiB", size.total_bytes() as f64 / 1024.0 / 1024.0),
        );
    }

    // ---- 基準（何も設定しない。アプリの現在の動作）----
    prime_index(&repo);
    // (b) 変更なしの list_changes
    measure_status(scenario, "基準", "変更なし", 0)?;

    // (c) 100 ファイル変更後の list_changes
    modify_100(&repo, layout, 1)?;
    measure_status(scenario, "基準", "100件変更", 100)?;

    // (d) 自動保存（一時インデックス）の作成時間。1 回目は作成、2 回目は同じ内容で作らない場合
    let (r, d) = timed(|| ops().auto_snapshot(&repo, SizeLimits::default()));
    match r? {
        AutoSnapshotOutcome::Created { .. } => {
            emit(
                scenario,
                "(d) 自動保存 作成",
                format!("{:.1}s", d.as_secs_f64()),
            );
        }
        other => panic!("自動保存が作られませんでした: {other:?}"),
    }
    let (r, d) = timed(|| ops().auto_snapshot(&repo, SizeLimits::default()));
    emit(
        scenario,
        "(d) 自動保存 同じ内容(作らない)",
        format!("{:.1}s ({:?})", d.as_secs_f64(), r?),
    );

    // (e) project_status 相当（sync_state + conflicts + list_changes + commit_count）
    for state in ["100件変更", "変更なし"] {
        if state == "変更なし" {
            ops().save(&repo, "100 件の変更を保存")?;
        }
        let mut samples = Vec::new();
        for _ in 0..3 {
            let (r, d) = timed(|| -> TestResult {
                let o = ops();
                o.sync_state(&repo)?;
                o.conflicts(&repo)?;
                o.list_changes(&repo)?;
                o.commit_count(&repo)?;
                Ok(())
            });
            r?;
            samples.push(ms(d));
        }
        emit_stats(
            scenario,
            &format!("(e) project_status相当/{state}"),
            &mut samples,
        );
    }
    // ここで 100 件の保存は済んでいる。保存の所要時間は別に測る
    modify_100(&repo, layout, 2)?;
    let (r, d) = timed(|| ops().save(&repo, "100 件の変更（基準）"));
    r?;
    emit(
        scenario,
        "参考/100件変更の保存(基準)",
        format!("{:.1}s", d.as_secs_f64()),
    );

    // ---- untracked cache ----
    let c = |args: &[&str]| runner().run_ok(&repo, args);
    c(&["config", "--local", "core.untrackedCache", "true"])?;
    prime_index(&repo);
    measure_status(scenario, "untrackedCache", "変更なし", 0)?;
    modify_100(&repo, layout, 3)?;
    measure_status(scenario, "untrackedCache", "100件変更", 100)?;
    let (r, d) = timed(|| ops().save(&repo, "100 件の変更（untrackedCache）"));
    r?;
    emit(
        scenario,
        "参考/100件変更の保存(untrackedCache)",
        format!("{:.1}s", d.as_secs_f64()),
    );

    // ---- fsmonitor（デーモン）+ untracked cache ----
    c(&["config", "--local", "core.fsmonitor", "true"])?;
    let (_, d) = timed(|| prime_index(&repo));
    emit(
        scenario,
        "参考/fsmonitor 有効化後の最初の2回のstatus",
        format!("{:.1}s", d.as_secs_f64()),
    );
    measure_status(scenario, "fsmonitor+untrackedCache", "変更なし", 0)?;
    modify_100(&repo, layout, 4)?;
    measure_status(scenario, "fsmonitor+untrackedCache", "100件変更", 100)?;
    let (r, d) = timed(|| ops().save(&repo, "100 件の変更（fsmonitor）"));
    r?;
    emit(
        scenario,
        "参考/100件変更の保存(fsmonitor)",
        format!("{:.1}s", d.as_secs_f64()),
    );
    // 後片付け: デーモンを止め、設定を元に戻す
    let _ = raw_git(&repo, &["fsmonitor--daemon", "stop"]);
    c(&["config", "--local", "--unset", "core.fsmonitor"])?;
    c(&["config", "--local", "--unset", "core.untrackedCache"])?;

    // (f) 履歴取得（ページ 100 件）。件数が足りなければ、小さな保存を重ねて作る
    if history_commits > 0 {
        let (r, d) = timed(|| -> TestResult {
            for n in 0..history_commits {
                write_file(&repo, layout, n % layout.files, 10 + n as u32)?;
                ops().save(&repo, &format!("履歴用の保存 {n}"))?;
            }
            Ok(())
        });
        r?;
        emit(
            scenario,
            "参考/履歴用の保存を重ねた時間",
            format!("{} 件 {:.1}s", history_commits, d.as_secs_f64()),
        );
    }
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut count = 0;
    for _ in 0..3 {
        let (r, d) = timed(|| ops().history_page(&repo, 0, 100));
        count = r?.len();
        first.push(ms(d));
        let (r, d) = timed(|| ops().history_page(&repo, 100, 100));
        let _ = r?;
        second.push(ms(d));
    }
    emit(scenario, "(f) 履歴 保存の総数", ops().commit_count(&repo)?);
    emit(scenario, "(f) 履歴 1ページ目の件数", count);
    emit_stats(scenario, "(f) 履歴 offset=0 limit=100", &mut first);
    emit_stats(scenario, "(f) 履歴 offset=100 limit=100", &mut second);

    // 後片付けの前に、フォルダが壊れていないことだけ確かめる
    assert_eq!(ops().list_changes(&repo)?.len(), 0);
    common::run_git(&repo, &["fsmonitor--daemon", "stop"]);
    Ok(())
}

/// 1 万ファイル（深さ 3〜4、日本語名を含む、一部は 1MB）
#[test]
#[ignore = "性能の計測。手動で実行する（ファイル先頭のコメント参照）"]
fn perf_many_files() -> TestResult {
    let files = env_usize("HIKAE_PERF_FILES", 10_000);
    let history = env_usize("HIKAE_PERF_HISTORY", 130);
    run_scenario(
        "1万ファイル",
        Layout {
            files,
            big_bytes: None,
        },
        history,
    )
}

/// 合計 5GB 相当（既定は 1,000 ファイル × 5MB）
#[test]
#[ignore = "性能の計測。手動で実行する（ファイル先頭のコメント参照）"]
fn perf_large_total() -> TestResult {
    let files = env_usize("HIKAE_PERF_LARGE_FILES", 1_000);
    let mb = env_usize("HIKAE_PERF_LARGE_MB", 5);
    run_scenario(
        "5GB相当",
        Layout {
            files,
            big_bytes: Some(mb * 1024 * 1024),
        },
        0,
    )
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
