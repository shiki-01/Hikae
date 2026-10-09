// git が使えるかの確認（設計書 5章 E17）。
//
// 起動時に、同梱の git（無ければ PATH 上の git）が実際に実行できるかを確かめる。
// git の呼び出しは必ず `GitRunner` を通す。`GitRunner` の許可リストは `--version` だけの呼び出しを
// 受け付けないため、リポジトリではない一時的な場所に対して `rev-parse --git-dir` を実行し、
// 「git が起動し、リポジトリではないと答えた」ことで使えると判断する（バージョンは取得できない）。

use core_git::{GitError, GitRunner};
use std::path::Path;

/// git の確認結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProbe {
    /// 起動でき、git として応答した
    Available,
    /// 実行ファイルが見つからない
    NotFound,
    /// 実行ファイルはあるが、起動できない・応答がおかしい（壊れている、実行できない、時間切れ）。
    /// 値は診断用の短い説明（標準エラー出力を含みうるため、画面の主文言には使わない）
    Broken(String),
}

impl GitProbe {
    /// git が使えるか
    pub fn is_available(&self) -> bool {
        matches!(self, GitProbe::Available)
    }
}

/// `runner` の git が使えるかを確かめる。
///
/// `scratch` は、実行の場所にするフォルダ（存在すること。通常は OS の一時フォルダ）。ここには何も
/// 書き込まない。`git rev-parse --git-dir` は読み取りだけで、リポジトリでなければ終了コード 128 と
/// 「not a git repository」を返す。リポジトリの中だった場合は 0 を返す。どちらも git の正常な応答。
pub fn probe_git(runner: &GitRunner, scratch: &Path) -> GitProbe {
    match runner.run(scratch, &["rev-parse", "--git-dir"]) {
        Ok(out) => {
            if out.code == 0 || is_not_a_repository(&out.stderr) {
                GitProbe::Available
            } else {
                GitProbe::Broken(format!(
                    "unexpected response (exit {}): {}",
                    out.code,
                    out.stderr.trim()
                ))
            }
        }
        Err(GitError::Spawn(e)) if e.kind() == std::io::ErrorKind::NotFound => GitProbe::NotFound,
        Err(e) => GitProbe::Broken(e.to_string()),
    }
}

/// 「リポジトリではない」という git の応答か
fn is_not_a_repository(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("not a git repository")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_not_a_repository_reply() {
        assert!(is_not_a_repository(
            "fatal: not a git repository (or any of the parent directories): .git"
        ));
        assert!(is_not_a_repository("FATAL: NOT A GIT REPOSITORY"));
        assert!(!is_not_a_repository(""));
        assert!(!is_not_a_repository("fatal: unable to read config file"));
    }

    #[test]
    fn only_available_counts_as_usable() {
        assert!(GitProbe::Available.is_available());
        assert!(!GitProbe::NotFound.is_available());
        assert!(!GitProbe::Broken("x".to_string()).is_available());
    }
}
