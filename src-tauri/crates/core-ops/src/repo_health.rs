// リポジトリの健全性と容量の検査（設計書 5章 E09・E10）。すべて読み取りのみ。
//
// - 破損の検査（E10）: 設計書の検出方法は `git fsck` だが、`fsck` は重く（履歴全体を読む）、許可リストにも
//   無い。そこで、起動時に毎回行える軽い検査として、許可済みのコマンドだけで「リポジトリとして読めるか」
//   「保存の記録（HEAD とそのルートのツリー）が読めるか」を確かめる。同期ソフトなどでファイルが欠けたり
//   壊れたりした典型的な場合は見つかるが、履歴の奥のオブジェクトの欠落までは検出できない。
// - 容量の検査（E09）: `git count-objects -v` の値から、`.git` の大きさを求める。

use crate::models::OpsError;
use core_git::GitRunner;
use std::path::Path;

/// 保存のデータが「大きくなっている」とみなす大きさ（1GiB。設計書 5章 E09）。これを**超える**と注意する
pub const REPO_SIZE_WARN_BYTES: u64 = 1024 * 1024 * 1024;

/// 破損が疑われる理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenReason {
    /// リポジトリとして読めない（`.git` の中身が欠けた、`HEAD` が壊れたなど）
    NotARepository,
    /// オブジェクトの保管場所を読めない
    ObjectsUnreadable,
    /// 保存済みのはずなのに、`HEAD` が最新の保存を指していない
    HeadMissing,
    /// `HEAD` の指す保存、またはそのルートのツリーが読めない
    HeadObjectMissing,
}

/// 検査の結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoHealth {
    Healthy,
    Broken(BrokenReason),
}

impl RepoHealth {
    pub fn is_broken(&self) -> bool {
        matches!(self, RepoHealth::Broken(_))
    }
}

/// 軽い検査でリポジトリが読めるかを確かめる。
///
/// `expect_commits` は、このプロジェクトにはすでに保存がある（アプリが最初の保存を記録している）か。
/// 真なのに `HEAD` が保存を指していなければ、記録が壊れたとみなす（保存がまだ無いプロジェクトの
/// 「空の `HEAD`」とは区別する）。
///
/// git が起動できない・時間切れなどの実行の失敗は、リポジトリの破損とは区別して `Err` で返す。
pub fn check_repo_health(
    runner: &GitRunner,
    repo: &Path,
    expect_commits: bool,
) -> Result<RepoHealth, OpsError> {
    // 1) リポジトリとして認識できるか（`HEAD` や設定が壊れていると、ここで失敗する）
    let git_dir = run_checked(runner, repo, &["rev-parse", "--git-dir"])?;
    if git_dir.code != 0 {
        return Ok(RepoHealth::Broken(BrokenReason::NotARepository));
    }

    // 2) オブジェクトの保管場所を数えられるか
    let counted = run_checked(runner, repo, &["count-objects", "-v"])?;
    if counted.code != 0 {
        return Ok(RepoHealth::Broken(BrokenReason::ObjectsUnreadable));
    }

    // 3) HEAD の指す保存と、そのルートのツリーが読めるか
    let head = run_checked(runner, repo, &["rev-parse", "--verify", "-q", "HEAD"])?;
    if head.code != 0 {
        // 保存がまだ無いプロジェクトでは、HEAD が何も指さないのが正常
        return Ok(if expect_commits {
            RepoHealth::Broken(BrokenReason::HeadMissing)
        } else {
            RepoHealth::Healthy
        });
    }
    let oid = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if oid.is_empty() {
        return Ok(RepoHealth::Broken(BrokenReason::HeadMissing));
    }
    let tree = format!("{oid}^{{tree}}");
    let readable = run_checked(runner, repo, &["cat-file", "-e", &tree])?;
    if readable.code != 0 {
        return Ok(RepoHealth::Broken(BrokenReason::HeadObjectMissing));
    }

    Ok(RepoHealth::Healthy)
}

/// 終了コードを問わずに実行し、git が起動できない失敗だけを `Err` にする。
fn run_checked(
    runner: &GitRunner,
    repo: &Path,
    args: &[&str],
) -> Result<core_git::GitOutput, OpsError> {
    // `run` は終了コードが 0 以外でも `Ok` で返す。`Err` は起動・時間切れ・許可リストの拒否だけ
    runner.run(repo, args).map_err(OpsError::from)
}

/// `git count-objects -v` の値（KiB 単位）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepoSize {
    /// バラバラのオブジェクト（loose）の大きさ
    pub loose_kib: u64,
    /// パック済みのオブジェクトの大きさ
    pub pack_kib: u64,
    /// 不要なファイルの大きさ
    pub garbage_kib: u64,
}

impl RepoSize {
    /// 合計の大きさ（バイト）
    pub fn total_bytes(&self) -> u64 {
        self.loose_kib
            .saturating_add(self.pack_kib)
            .saturating_add(self.garbage_kib)
            .saturating_mul(1024)
    }

    /// 警告の目安（1GiB）を超えているか
    pub fn exceeds_warning(&self) -> bool {
        self.total_bytes() > REPO_SIZE_WARN_BYTES
    }
}

/// `count-objects -v` の出力（`LC_ALL=C` の英語）を解析する。読めない行は無視する。
pub fn parse_count_objects(stdout: &str) -> RepoSize {
    let mut size = RepoSize::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "size" => size.loose_kib = value,
            "size-pack" => size.pack_kib = value,
            "size-garbage" => size.garbage_kib = value,
            _ => {}
        }
    }
    size
}

/// リポジトリ（`.git`）の大きさを調べる（読み取りのみ）。
pub fn repo_size(runner: &GitRunner, repo: &Path) -> Result<RepoSize, OpsError> {
    let out = runner.run_ok(repo, &["count-objects", "-v"])?;
    Ok(parse_count_objects(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "count: 3\nsize: 12\nin-pack: 100\npacks: 1\nsize-pack: 2048\nprune-packable: 0\ngarbage: 1\nsize-garbage: 4\n";

    #[test]
    fn parses_count_objects_output() {
        let size = parse_count_objects(SAMPLE);
        assert_eq!(size.loose_kib, 12);
        assert_eq!(size.pack_kib, 2048);
        assert_eq!(size.garbage_kib, 4);
        assert_eq!(size.total_bytes(), (12 + 2048 + 4) * 1024);
    }

    #[test]
    fn unreadable_lines_are_ignored() {
        let size = parse_count_objects("size: abc\nnonsense\nsize-pack: 5\n: 7\n");
        assert_eq!(size.loose_kib, 0);
        assert_eq!(size.pack_kib, 5);
        assert_eq!(parse_count_objects(""), RepoSize::default());
    }

    #[test]
    fn warning_is_raised_only_above_one_gibibyte() {
        let gib_kib = REPO_SIZE_WARN_BYTES / 1024;
        let exactly = RepoSize {
            loose_kib: 0,
            pack_kib: gib_kib,
            garbage_kib: 0,
        };
        assert!(!exactly.exceeds_warning());
        let above = RepoSize {
            loose_kib: 1,
            pack_kib: gib_kib,
            garbage_kib: 0,
        };
        assert!(above.exceeds_warning());
        // 内訳が分かれていても合計で判定する
        let split = RepoSize {
            loose_kib: gib_kib / 2,
            pack_kib: gib_kib / 2,
            garbage_kib: 1,
        };
        assert!(split.exceeds_warning());
        assert!(!RepoSize::default().exceeds_warning());
    }

    #[test]
    fn huge_values_do_not_overflow() {
        let huge = RepoSize {
            loose_kib: u64::MAX,
            pack_kib: u64::MAX,
            garbage_kib: u64::MAX,
        };
        assert!(huge.exceeds_warning());
    }
}
