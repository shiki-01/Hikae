// 画面の時点 ID を、読み取りに使う rev に解決する（読み取りのみ。設計書 3.3 S4、競合ダイアログの比較）。
//
// - `current` と `work` は作業フォルダ（`WORK_REV`）
// - `this` は HEAD（この PC の版）、`cloud` は MERGE_HEAD（クラウドの版）。競合の途中でだけ MERGE_HEAD がある
// - それ以外は 16 進の短縮 ID（4〜64 桁）。完全な OID に解決する
// - 版が無いとき（HEAD や MERGE_HEAD が無い、ID が解決できない）は `Io(NotFound)`。
//   形が不正な値（16 進でない、知らない語）は `InvalidInput`
// - git は `rev-parse --verify --quiet` だけを使う（許可リストは変更しない）。作業フォルダには書かない

use crate::blob::WORK_REV;
use crate::models::OpsError;
use core_git::GitRunner;
use std::path::Path;

/// 作業フォルダを指す画面の時点 ID（`work` と同じ意味）
const CURRENT_POINT: &str = "current";
/// この PC の版（HEAD）
const THIS_POINT: &str = "this";
/// クラウドの版（MERGE_HEAD）
const CLOUD_POINT: &str = "cloud";

/// 画面の時点 ID を読み取りの rev にする（規則はモジュールの先頭に書いたもの）
pub(crate) fn resolve_point(
    runner: &GitRunner,
    repo: &Path,
    point: &str,
) -> Result<String, OpsError> {
    match point {
        CURRENT_POINT | WORK_REV => Ok(WORK_REV.to_string()),
        THIS_POINT => commit_or_not_found(runner, repo, "HEAD"),
        CLOUD_POINT => commit_or_not_found(runner, repo, "MERGE_HEAD"),
        _ if is_short_hex(point) => commit_or_not_found(runner, repo, point),
        _ => Err(OpsError::InvalidInput(
            "point is not a known version".to_string(),
        )),
    }
}

/// 作業フォルダを指す時点 ID か（`current` と `work`）
pub(crate) fn is_working_point(point: &str) -> bool {
    point == CURRENT_POINT || point == WORK_REV
}

/// 4 から 64 桁の 16 進か（完全な OID も含む）
fn is_short_hex(rev: &str) -> bool {
    (4..=64).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 指す commit の完全な OID。存在しなければ `Io(NotFound)`（版が無い、という意味）
fn commit_or_not_found(runner: &GitRunner, repo: &Path, rev: &str) -> Result<String, OpsError> {
    let spec = format!("{rev}^{{commit}}");
    let out = runner.run(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.code != 0 || oid.is_empty() {
        return Err(OpsError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )));
    }
    Ok(oid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hex_is_four_to_sixty_four_digits() {
        assert!(is_short_hex("abcd"));
        assert!(is_short_hex(&"0".repeat(64)));
        assert!(!is_short_hex("abc"));
        assert!(!is_short_hex(&"a".repeat(65)));
        assert!(!is_short_hex("main"));
        assert!(!is_short_hex("HEAD~1"));
    }

    #[test]
    fn working_points_are_current_and_work_only() {
        assert!(is_working_point("current"));
        assert!(is_working_point("work"));
        assert!(!is_working_point("this"));
        assert!(!is_working_point("cloud"));
        assert!(!is_working_point("abcd"));
    }
}
