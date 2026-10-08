// Git 操作の基盤 crate。GitRunner を通じてすべての git 呼び出しを統制する。

mod bundled;
mod parser;
mod runner;
mod validation;

pub use bundled::{BundledEnv, GitSource, Layout, GIT_PATH_ENV};
pub use parser::{
    parse_status_v2, BranchInfo, ParseError, StatusCode, StatusEntry, StatusKind, StatusV2,
};
pub use runner::{GitOutput, GitRunner};
pub use validation::{validate, Rejection};

use thiserror::Error;

pub const APP_REF_NAMESPACE: &str = "refs/hikae/";

/// Git の実行に関連するエラー型
#[derive(Error, Debug)]
pub enum GitError {
    /// コマンドが許可リストに一致しなかった
    #[error("command rejected: {}", .0.reason)]
    Rejected(Rejection),

    /// プロセス生成に失敗
    #[error("failed to spawn git process: {}", .0)]
    Spawn(#[from] std::io::Error),

    /// コマンド実行がタイムアウト
    #[error("git command timed out (args: {})", .args_summary)]
    Timeout { args_summary: String },

    /// コマンドが 0 以外で終了
    #[error("git command failed with exit code {}: {} (args: {})", .code, .stderr, .args_summary)]
    Failed {
        code: i32,
        stderr: String,
        args_summary: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_ref_namespace() {
        assert_eq!(APP_REF_NAMESPACE, "refs/hikae/");
    }
}
