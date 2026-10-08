use crate::bundled::{self, BundledEnv, GitSource, Layout, Resolved, GIT_PATH_ENV};
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
    /// ネットワーク通信（clone / fetch / push / ls-remote）にだけ渡す credential helper。
    /// アプリ自身を指す `!<コマンド>` 形式。トークンは含まれず、git が実行時に helper へ問い合わせる
    credential_helper: Option<String>,
    /// 同梱構成の git を使うときに子プロセスへ足す設定（PATH の先頭、GIT_EXEC_PATH など）
    bundled: Option<BundledEnv>,
    /// git の入手元（ログ・診断用）
    source: GitSource,
}

impl GitRunner {
    /// git の絶対パスを指定して新しい GitRunner を作成する
    pub fn new(git_path: impl Into<PathBuf>) -> Self {
        GitRunner {
            git_path: git_path.into(),
            timeout: Duration::from_secs(60),
            credential_helper: None,
            bundled: None,
            source: GitSource::Explicit,
        }
    }

    fn from_resolved(r: Resolved) -> Self {
        GitRunner {
            git_path: r.git_path,
            timeout: Duration::from_secs(60),
            credential_helper: None,
            bundled: r.bundled,
            source: r.source,
        }
    }

    /// git を探して GitRunner を作成する（テスト・開発用）。
    ///
    /// 環境変数 `HIKAE_GIT_PATH` が存在するファイルを指していればそれを使い、無ければ PATH 上の git を使う。
    pub fn from_path_env() -> Self {
        let explicit = std::env::var_os(GIT_PATH_ENV);
        Self::from_resolved(bundled::resolve_without_resources(
            explicit.as_deref(),
            Layout::native(),
        ))
    }

    /// 同梱の git を優先して GitRunner を作成する。
    ///
    /// 優先順位: (1) 環境変数 `HIKAE_GIT_PATH`、(2) `<resource_dir>/git/` 以下の同梱 git、(3) PATH 上の git。
    /// 同梱 git を使うときは、PATH の先頭・`GIT_EXEC_PATH`・`GIT_TEMPLATE_DIR` を子プロセスにだけ渡す。
    pub fn bundled(resource_dir: &Path) -> Self {
        let explicit = std::env::var_os(GIT_PATH_ENV);
        Self::from_resolved(bundled::resolve(
            Some(resource_dir),
            explicit.as_deref(),
            Layout::native(),
        ))
    }

    /// git の入手元（明示指定・同梱・PATH）
    pub fn source(&self) -> GitSource {
        self.source
    }

    /// タイムアウト時間を設定する
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// ネットワーク通信の認証に使う credential helper を設定する。
    ///
    /// `!<コマンド>` 形式の文字列を渡す。clone / fetch / push / ls-remote の実行時だけ、
    /// `-c credential.helper=`（既存の helper を無効化）と `-c credential.helper=<helper>` を先頭に付ける。
    /// 空文字や制御文字を含む値は設定せず無視する（認証なしで実行され、認証エラーとして失敗する）。
    /// ユーザーのグローバル git 設定には何も書き込まない。
    pub fn with_credential_helper(mut self, helper: impl Into<String>) -> Self {
        let helper = helper.into();
        if !helper.is_empty() && !helper.chars().any(char::is_control) {
            self.credential_helper = Some(helper);
        }
        self
    }

    /// git コマンドを実行する
    pub fn run(&self, repo: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        self.run_with_env(repo, args, &[])
    }

    /// 終了コードが 0 以外なら `GitError::Failed` にする版
    pub fn run_ok(&self, repo: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        self.run_allow_codes(repo, args, &[0])
    }

    /// 指定した終了コードのみ成功とみなす版（例: merge の競合は終了コード 1）
    pub fn run_allow_codes(
        &self,
        repo: &Path,
        args: &[&str],
        allowed: &[i32],
    ) -> Result<GitOutput, GitError> {
        let out = self.run(repo, args)?;
        if allowed.contains(&out.code) {
            Ok(out)
        } else {
            Err(GitError::Failed {
                code: out.code,
                stderr: out.stderr,
                args_summary: args.join(" "),
            })
        }
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

        // ネットワーク通信にだけ credential helper を付ける。付与後の引数列も検証にかける
        let extra_config = credential_config_args(
            self.credential_helper.as_deref(),
            subcommand_of(args).unwrap_or_default(),
        );
        if !extra_config.is_empty() {
            let combined: Vec<&str> = extra_config
                .iter()
                .map(String::as_str)
                .chain(args.iter().copied())
                .collect();
            validate(&combined).map_err(GitError::Rejected)?;
        }

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

        // 同梱 git のときは、同梱のフォルダを PATH の先頭に足し、GIT_EXEC_PATH などを渡す。
        // 子プロセスにだけ渡す設定で、ユーザーの環境やグローバル git 設定は変更しない
        if let Some(env) = &self.bundled {
            if let Some(path) = bundled::prepend_path(&env.path_dirs, std::env::var_os("PATH")) {
                cmd.env("PATH", path);
            }
            for (key, value) in bundled::extra_env(env) {
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
        if let Some(subcmd) = subcommand_of(args) {
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

        // 引数を追加（credential helper の設定は git のグローバルオプションとして先頭に置く）
        for arg in &extra_config {
            cmd.arg(arg);
        }
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

/// 先頭の `-c key=value` を読み飛ばして、サブコマンド名を返す。
fn subcommand_of<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        if arg == "-c" {
            i += 2;
        } else if arg.starts_with("-c") {
            i += 1;
        } else {
            return Some(arg);
        }
    }
    None
}

/// credential helper を渡す対象のサブコマンド（ネットワーク通信を行うもの）
fn needs_credentials(subcmd: &str) -> bool {
    matches!(subcmd, "clone" | "fetch" | "push" | "ls-remote")
}

/// credential helper を有効にする `-c` 引数列を作る。対象外のサブコマンドや helper 未設定なら空。
/// 先に空値で既存の helper を無効化し、そのあとアプリの helper だけを指定する。
fn credential_config_args(helper: Option<&str>, subcmd: &str) -> Vec<String> {
    match helper {
        Some(h) if needs_credentials(subcmd) => vec![
            "-c".to_string(),
            "credential.helper=".to_string(),
            "-c".to_string(),
            format!("credential.helper={h}"),
        ],
        _ => Vec::new(),
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
    fn subcommand_skips_leading_config_options() {
        assert_eq!(subcommand_of(&["status", "-z"]), Some("status"));
        assert_eq!(
            subcommand_of(&["-c", "core.hooksPath=", "commit", "-m", "x"]),
            Some("commit")
        );
        assert_eq!(
            subcommand_of(&["-ccore.hooksPath=", "commit"]),
            Some("commit")
        );
        assert_eq!(subcommand_of(&[]), None);
    }

    #[test]
    fn credential_helper_is_added_only_to_network_commands() {
        let helper = "!'C:/app/hikae.exe' credential";
        for sub in ["clone", "fetch", "push", "ls-remote"] {
            let args = credential_config_args(Some(helper), sub);
            assert_eq!(
                args,
                vec![
                    "-c".to_string(),
                    "credential.helper=".to_string(),
                    "-c".to_string(),
                    format!("credential.helper={helper}"),
                ]
            );
            // 付与後の引数列も許可リストを通ること
            let mut full: Vec<&str> = args.iter().map(String::as_str).collect();
            full.push(sub);
            assert!(validate(&full).is_ok(), "{sub}");
        }
        for sub in ["status", "commit", "merge", "add", "config"] {
            assert!(credential_config_args(Some(helper), sub).is_empty());
        }
        assert!(credential_config_args(None, "fetch").is_empty());
    }

    #[test]
    fn invalid_credential_helper_is_ignored() {
        let valid = GitRunner::new("git").with_credential_helper("!'x' credential");
        assert!(valid.credential_helper.is_some());
        let empty = GitRunner::new("git").with_credential_helper("");
        assert!(empty.credential_helper.is_none());
        let control = GitRunner::new("git").with_credential_helper("!x\ny");
        assert!(control.credential_helper.is_none());
    }

    #[test]
    fn test_gitrunner_with_timeout() {
        let runner = GitRunner::new("git").with_timeout(Duration::from_secs(30));
        assert_eq!(runner.timeout, Duration::from_secs(30));
    }
}
