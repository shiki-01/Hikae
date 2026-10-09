// 他のアプリが同じプロジェクトを操作中かの判定（設計書 5章 E14）。
//
// git は、インデックスを書き換える間 `.git/index.lock` を作る。このファイルが残っていると、保存や
// 取り込みは失敗する。残っている理由は 2 つある。
//
// - 別のアプリ（他の Git ツール、エディタの拡張機能など）が、いま操作している: 終わるまで待つ
// - 前回の操作が途中で終わった（アプリや PC が強制終了した）: 待っても消えない。利用者に案内する
//
// この 2 つは、ロックファイルの古さと、git のプロセスが動いているかで見分ける。
// 判定は時刻とプロセスの有無を引数で受け取る純関数にして、実時間に依存せずにテストできるようにする。
// **ロックファイルは自動では削除しない**（動いている操作を壊すおそれがあるため。案内だけを行う）。

use crate::models::OpsError;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// ロックがこれ以上古く、git のプロセスも動いていなければ、前回の操作が途中で終わったとみなす（10 分）
pub const STALE_LOCK_AGE: Duration = Duration::from_secs(10 * 60);

/// 他の操作の完了を待つ既定の計画（0.5 秒おきに 20 秒まで）。
/// 状態を変える操作の前に、待ってもロックが残っていれば、待ちきれなかったとして断る
pub const DEFAULT_WAIT: WaitPlan = WaitPlan {
    max_polls: 40,
    interval: Duration::from_millis(500),
};

/// ロックの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexLockState {
    /// ロックは無い
    Free,
    /// 他の操作が動いている（または動いている可能性がある）。待つ
    Busy,
    /// ロックが古く、git のプロセスも動いていない。前回の操作が途中で終わった
    Stale,
}

/// 待ち方の計画
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitPlan {
    /// ロックが残っているとき、確かめ直す最大の回数
    pub max_polls: u32,
    /// 確かめ直す間隔
    pub interval: Duration,
}

/// `index.lock` の場所
pub fn index_lock_path(repo: &Path) -> PathBuf {
    repo.join(".git").join("index.lock")
}

/// git の標準エラー出力が、`index.lock` が残っていることを示しているか。
/// 例: `fatal: Unable to create '.../.git/index.lock': File exists.`
pub(crate) fn is_index_lock_message(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();
    text.contains("index.lock")
        && (text.contains("file exists") || text.contains("another git process"))
}

/// ロックの古さ。更新時刻が未来（時計のずれ）のときは 0 とする。
fn lock_age(modified: SystemTime, now: SystemTime) -> Duration {
    now.duration_since(modified).unwrap_or(Duration::ZERO)
}

/// ロックの状態を判定する（純関数）。
///
/// - `lock_modified`: ロックファイルの更新時刻。ロックが無ければ `None`
/// - `now`: 現在時刻
/// - `git_running`: git のプロセスが動いているか。ロックが `STALE_LOCK_AGE` 以上古いときだけ呼ぶ
///   （新しいロックのたびにプロセスの一覧を調べないため）
pub fn assess_index_lock(
    lock_modified: Option<SystemTime>,
    now: SystemTime,
    git_running: impl FnOnce() -> bool,
) -> IndexLockState {
    let Some(modified) = lock_modified else {
        return IndexLockState::Free;
    };
    if lock_age(modified, now) >= STALE_LOCK_AGE && !git_running() {
        IndexLockState::Stale
    } else {
        IndexLockState::Busy
    }
}

/// ロックファイルの更新時刻。無ければ `None`。リンクはたどらない。
pub fn index_lock_modified(repo: &Path) -> Result<Option<SystemTime>, OpsError> {
    match std::fs::symlink_metadata(index_lock_path(repo)) {
        Ok(meta) => Ok(Some(meta.modified()?)),
        // `.git` がフォルダでない（gitfile）場合も、ここでは「ロックは無い」とする
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(OpsError::from(e)),
    }
}

/// 状態を変える操作の前に、他の操作が `index.lock` を持っていないことを確かめる。
///
/// ロックが残っていれば、`plan` に従って確かめ直しながら待つ。待つ間は `sleep` を呼ぶ
/// （テストでは実際には待たない関数を渡す）。
///
/// - 解放された: `Ok(())`
/// - 古く、git のプロセスも動いていない: `Err(IndexLocked { stale: true })`（待たずに断る）
/// - 待ちきれなかった: `Err(IndexLocked { stale: false })`
///
/// ロックファイルは削除しない。
pub fn ensure_index_free(
    repo: &Path,
    plan: WaitPlan,
    now: &dyn Fn() -> SystemTime,
    git_running: &dyn Fn() -> bool,
    sleep: &mut dyn FnMut(Duration),
) -> Result<(), OpsError> {
    let mut polls = 0;
    loop {
        match assess_index_lock(index_lock_modified(repo)?, now(), git_running) {
            IndexLockState::Free => return Ok(()),
            IndexLockState::Stale => return Err(OpsError::IndexLocked { stale: true }),
            IndexLockState::Busy => {
                if polls >= plan.max_polls {
                    return Err(OpsError::IndexLocked { stale: false });
                }
                polls += 1;
                sleep(plan.interval);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn no_lock_is_free_and_never_asks_about_processes() {
        let state = assess_index_lock(None, at(1_000), || panic!("プロセスは調べない"));
        assert_eq!(state, IndexLockState::Free);
    }

    #[test]
    fn a_fresh_lock_is_busy_without_asking_about_processes() {
        // 10 分未満のロックは、プロセスを調べるまでもなく待つ
        let state = assess_index_lock(Some(at(1_000)), at(1_000 + 599), || {
            panic!("プロセスは調べない")
        });
        assert_eq!(state, IndexLockState::Busy);
    }

    #[test]
    fn an_old_lock_is_stale_only_when_no_git_process_is_running() {
        let old = at(1_000);
        let now = at(1_000 + STALE_LOCK_AGE.as_secs());
        // ちょうど 10 分で古いとみなす
        assert_eq!(
            assess_index_lock(Some(old), now, || false),
            IndexLockState::Stale
        );
        // git のプロセスが動いているなら、長い操作の途中かもしれないので待つ
        assert_eq!(
            assess_index_lock(Some(old), now, || true),
            IndexLockState::Busy
        );
        // 1 秒足りなければ古くない
        assert_eq!(
            assess_index_lock(Some(old), at(1_000 + STALE_LOCK_AGE.as_secs() - 1), || {
                false
            }),
            IndexLockState::Busy
        );
    }

    #[test]
    fn a_lock_from_the_future_is_treated_as_fresh() {
        let state = assess_index_lock(Some(at(5_000)), at(1_000), || false);
        assert_eq!(state, IndexLockState::Busy);
    }

    #[test]
    fn detects_the_git_message_for_a_leftover_lock() {
        for msg in [
            "fatal: Unable to create 'C:/work/.git/index.lock': File exists.\n\nAnother git process seems to be running in this repository",
            "fatal: Unable to create '/home/a/work/.git/index.lock': File exists.",
            "FATAL: UNABLE TO CREATE '.GIT/INDEX.LOCK': FILE EXISTS.",
        ] {
            assert!(is_index_lock_message(msg), "{msg}");
        }
        for msg in [
            "",
            "fatal: not a git repository",
            "fatal: Unable to create '.git/refs/heads/main.lock': File exists.",
            "error: unable to unlink old 'a.docx': Permission denied",
        ] {
            assert!(!is_index_lock_message(msg), "{msg}");
        }
    }

    #[test]
    fn git_failure_becomes_index_locked() {
        let e = core_git::GitError::Failed {
            code: 128,
            stderr: "fatal: Unable to create '.git/index.lock': File exists.".to_string(),
            args_summary: "add -A".to_string(),
        };
        assert!(matches!(
            OpsError::from(e),
            OpsError::IndexLocked { stale: false }
        ));
    }

    fn make_repo_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tmp.path().join(".git")).expect("mkdir .git");
        tmp
    }

    #[test]
    fn modified_time_is_read_from_the_lock_file() {
        let tmp = make_repo_dir();
        assert_eq!(index_lock_modified(tmp.path()).expect("read"), None);
        std::fs::write(index_lock_path(tmp.path()), b"").expect("write lock");
        assert!(index_lock_modified(tmp.path()).expect("read").is_some());
        // `.git` が無いフォルダでもエラーにしない
        let no_git = tempfile::tempdir().expect("tempdir");
        assert_eq!(index_lock_modified(no_git.path()).expect("read"), None);
    }

    #[test]
    fn waiting_returns_as_soon_as_the_lock_is_released() {
        let tmp = make_repo_dir();
        let lock = index_lock_path(tmp.path());
        std::fs::write(&lock, b"").expect("write lock");
        let now = SystemTime::now();

        let mut slept = 0u32;
        let lock_for_sleep = lock.clone();
        let result = ensure_index_free(
            tmp.path(),
            WaitPlan {
                max_polls: 5,
                interval: Duration::from_millis(1),
            },
            &move || now,
            &|| panic!("新しいロックではプロセスを調べない"),
            // 2 回目に待ったところで、他のアプリがロックを解放したことにする
            &mut |_| {
                slept += 1;
                if slept == 2 {
                    std::fs::remove_file(&lock_for_sleep).expect("release");
                }
            },
        );
        assert!(result.is_ok());
        assert_eq!(slept, 2);
    }

    #[test]
    fn waiting_gives_up_after_the_planned_polls_and_keeps_the_lock() {
        let tmp = make_repo_dir();
        let lock = index_lock_path(tmp.path());
        std::fs::write(&lock, b"").expect("write lock");
        let now = SystemTime::now();

        let mut slept = 0u32;
        let result = ensure_index_free(
            tmp.path(),
            WaitPlan {
                max_polls: 3,
                interval: Duration::from_millis(1),
            },
            &move || now,
            &|| true,
            &mut |_| slept += 1,
        );
        assert!(matches!(
            result,
            Err(OpsError::IndexLocked { stale: false })
        ));
        assert_eq!(slept, 3);
        // ロックファイルは削除しない
        assert!(lock.exists());
    }

    #[test]
    fn a_stale_lock_is_refused_without_waiting_and_is_not_deleted() {
        let tmp = make_repo_dir();
        let lock = index_lock_path(tmp.path());
        std::fs::write(&lock, b"").expect("write lock");
        // 現在時刻を 1 時間進めて、ロックを古く見せる
        let later = SystemTime::now() + Duration::from_secs(3_600);

        let mut slept = 0u32;
        let result = ensure_index_free(
            tmp.path(),
            DEFAULT_WAIT,
            &move || later,
            &|| false,
            &mut |_| slept += 1,
        );
        assert!(matches!(result, Err(OpsError::IndexLocked { stale: true })));
        assert_eq!(slept, 0);
        assert!(lock.exists());
    }

    #[test]
    fn an_old_lock_with_a_running_git_process_is_waited_for() {
        let tmp = make_repo_dir();
        std::fs::write(index_lock_path(tmp.path()), b"").expect("write lock");
        let later = SystemTime::now() + Duration::from_secs(3_600);

        let mut slept = 0u32;
        let result = ensure_index_free(
            tmp.path(),
            WaitPlan {
                max_polls: 2,
                interval: Duration::from_millis(1),
            },
            &move || later,
            &|| true,
            &mut |_| slept += 1,
        );
        assert!(matches!(
            result,
            Err(OpsError::IndexLocked { stale: false })
        ));
        assert_eq!(slept, 2);
    }

    #[test]
    fn no_lock_passes_immediately() {
        let tmp = make_repo_dir();
        let mut slept = 0u32;
        let result = ensure_index_free(
            tmp.path(),
            DEFAULT_WAIT,
            &SystemTime::now,
            &|| panic!("ロックが無ければプロセスは調べない"),
            &mut |_| slept += 1,
        );
        assert!(result.is_ok());
        assert_eq!(slept, 0);
    }
}
