use crate::validation::validate;
use crate::GitError;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// git コマンドの実行結果
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub code: i32,
}

/// GitRunner は git コマンドを安全に実行するためのランナー
pub struct GitRunner {
    git_path: PathBuf,
    timeout: Duration,
}

impl GitRunner {
    /// git の絶対パスを指定して新しい GitRunner を作成する
    pub fn new(git_path: impl Into<PathBuf>) -> Self {
        GitRunner {
            git_path: git_path.into(),
            timeout: Duration::from_secs(60),
        }
    }

    /// PATH から git を探して GitRunner を作成する（テスト・開発用）
    pub fn from_path_env() -> Self {
        GitRunner {
            git_path: PathBuf::from("git"),
            timeout: Duration::from_secs(60),
        }
    }

    /// タイムアウト時間を設定する
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// git コマンドを実行する
    pub fn run(&self, repo: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        self.run_with_env(repo, args, &[])
    }

    /// git コマンドを環境変数付きで実行する
    pub fn run_with_env(
        &self,
        repo: &Path,
        args: &[&str],
        extra_env: &[(&str, &OsStr)],
    ) -> Result<GitOutput, GitError> {
        // 引数を検証する（実行前に拒否）
        validate(args).map_err(GitError::Rejected)?;

        // extra_env の検証
        for (key, _) in extra_env {
            match *key {
                "GIT_INDEX_FILE"
                | "GIT_AUTHOR_NAME"
                | "GIT_AUTHOR_EMAIL"
                | "GIT_AUTHOR_DATE"
                | "GIT_COMMITTER_NAME"
                | "GIT_COMMITTER_EMAIL"
                | "GIT_COMMITTER_DATE" => {}
                _ => {
                    return Err(GitError::Rejected(crate::validation::Rejection {
                        reason: format!("env var '{}' not allowed", key),
                    }))
                }
            }
        }

        let args_summary = args.join(" ");

        // init と clone の場合は repo をカレントディレクトリにする
        let use_c_flag = !args.is_empty() && args[0] != "init" && args[0] != "clone";

        let mut cmd = Command::new(&self.git_path);

        // 親プロセスの環境変数をすべて除去し、動作に必要なものだけ引き継ぐ（GIT_* は引き継がない）
        cmd.env_clear();
        for key in ["PATH", "SystemRoot", "SYSTEMDRIVE", "TEMP", "TMP", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }

        // 必須の環境変数を設定
        if cfg!(windows) {
            cmd.env("GIT_CONFIG_GLOBAL", "NUL");
        } else {
            cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
        }

        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("LC_ALL", "C");

        // 読み取り系のコマンドの場合は GIT_OPTIONAL_LOCKS を設定
        if let Some(subcmd) = args.first() {
            if is_read_only_command(subcmd) {
                cmd.env("GIT_OPTIONAL_LOCKS", "0");
            }
        }

        // extra_env を設定
        for (key, value) in extra_env {
            cmd.env(key, value);
        }

        // -C フラグを追加
        if use_c_flag {
            cmd.arg("-C");
            cmd.arg(repo);
        } else {
            // init/clone の場合はカレントディレクトリに repo を設定
            cmd.current_dir(repo);
        }

        // 引数を追加
        for arg in args {
            cmd.arg(arg);
        }

        // Windows ではコンソール窓を出さない
        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }

        // stdout と stderr をキャプチャ
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(GitError::Spawn)?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // パイプは別スレッドで読み切る（バッファが詰まって子が止まるのを防ぐ）
        fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
            thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buf);
                }
                buf
            })
        }
        let stdout_thread = drain(stdout);
        let stderr_thread = drain(stderr);

        // タイムアウト処理
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // プロセスが終了した
                    let stdout_data = stdout_thread.join().unwrap_or_default();
                    let stderr_data = stderr_thread.join().unwrap_or_default();

                    let stderr_str = String::from_utf8_lossy(&stderr_data).into_owned();
                    let code = status.code().unwrap_or(-1);

                    return Ok(GitOutput {
                        stdout: stdout_data,
                        stderr: stderr_str,
                        code,
                    });
                }
                Ok(None) => {
                    // プロセスはまだ実行中
                    if start.elapsed() > self.timeout {
                        // タイムアウト。プロセスを kill する
                        let _ = child.kill();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(GitError::Timeout { args_summary });
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    // エラー
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(GitError::Spawn(e));
                }
            }
        }
    }
}

fn is_read_only_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "status"
            | "diff"
            | "log"
            | "show"
            | "ls-files"
            | "ls-tree"
            | "cat-file"
            | "rev-parse"
            | "rev-list"
            | "for-each-ref"
            | "merge-base"
            | "show-ref"
            | "symbolic-ref"
            | "check-ignore"
            | "ls-remote"
            | "count-objects"
            | "branch"
            | "remote"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_command() {
        assert!(is_read_only_command("status"));
        assert!(is_read_only_command("log"));
        assert!(!is_read_only_command("commit"));
        assert!(!is_read_only_command("push"));
    }

    #[test]
    fn test_gitrunner_new() {
        let runner = GitRunner::new("/usr/bin/git");
        assert_eq!(runner.git_path, PathBuf::from("/usr/bin/git"));
        assert_eq!(runner.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_gitrunner_with_timeout() {
        let runner = GitRunner::new("git").with_timeout(Duration::from_secs(30));
        assert_eq!(runner.timeout, Duration::from_secs(30));
    }
}
