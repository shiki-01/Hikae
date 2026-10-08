// フォルダが見つからないプロジェクトの付け替え先の確認（設計書 5章 E11）。
// 確認のみを行い、ファイルは一切変更しない。登録パスの更新は呼び出し側（app）が行う。

use crate::models::*;
use core_git::GitRunner;
use std::path::Path;

/// リモート URL を比較用の形にそろえる。
/// 方式（https / ssh / scp 形式）・認証情報・末尾の `.git` と `/`・大文字小文字の違いを無視する。
pub(crate) fn normalize_remote_url(url: &str) -> String {
    let mut s = url.trim().to_ascii_lowercase();
    for scheme in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest.to_string();
            break;
        }
    }
    // `user@host/...`、`user:token@host/...` の認証情報部分を除く
    if let Some(at) = s.find('@') {
        let slash = s.find('/').unwrap_or(s.len());
        if at < slash || !s[..at].contains('/') {
            s = s[at + 1..].to_string();
        }
    }
    // scp 形式（host:owner/repo）を host/owner/repo にそろえる
    if let Some(colon) = s.find(':') {
        let slash = s.find('/').unwrap_or(s.len());
        if colon < slash {
            let (host, rest) = s.split_at(colon);
            s = format!("{host}/{}", &rest[1..]);
        }
    }
    while s.ends_with('/') {
        s.pop();
    }
    if let Some(stripped) = s.strip_suffix(".git") {
        s = stripped.to_string();
    }
    s
}

/// リポジトリの初期 commit（親を持たない commit）の OID を返す。まだ保存が無ければ None。
/// 履歴に初期 commit が複数ある場合は、`rev-list` の出力で最後（最も古い側）のものを返す。
pub(crate) fn initial_commit(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    Ok(root_commits(runner, repo)?.pop())
}

/// 親を持たない commit の OID の一覧。`rev-list --max-parents=0` は許可リストに無いため、
/// 許可済みの `rev-list --parents` の出力から親の無い行を選ぶ。
fn root_commits(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, OpsError> {
    let head = runner.run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if head.code != 0 {
        return Ok(Vec::new());
    }
    let out = runner.run_ok(repo, &["rev-list", "--parents", "HEAD"])?;
    Ok(parse_root_commits(&String::from_utf8_lossy(&out.stdout)))
}

/// `rev-list --parents` の出力（`<oid> <親oid>...` の行）から、親の無い commit を選ぶ。
fn parse_root_commits(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut ids = line.split_whitespace();
            let oid = ids.next()?;
            ids.next().is_none().then(|| oid.to_string())
        })
        .collect()
}

/// 付け替え先のフォルダが、登録済みプロジェクトと同じリポジトリか確認する。
///
/// - フォルダがリポジトリの最上位でなければ `NotARepository`
/// - 登録済みの保存先 URL（`expected_remote`）があれば、付け替え先の `origin` の URL と照合する
/// - 保存先 URL が無ければ、登録済みの初期 commit（`expected_initial_commit`）が
///   付け替え先の履歴にあるかで照合する
/// - どちらも登録されていなければ照合できないので `CannotVerify`
pub(crate) fn check_relocation(
    runner: &GitRunner,
    new_path: &Path,
    expected_remote: Option<&str>,
    expected_initial_commit: Option<&str>,
) -> Result<RelocateCheck, OpsError> {
    if !new_path.is_dir() {
        return Ok(RelocateCheck::NotARepository);
    }
    let top = runner.run(new_path, &["rev-parse", "--show-toplevel"])?;
    if top.code != 0 {
        return Ok(RelocateCheck::NotARepository);
    }
    let top_path = String::from_utf8_lossy(&top.stdout).trim().to_string();
    let is_root = match (Path::new(&top_path).canonicalize(), new_path.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !is_root {
        return Ok(RelocateCheck::NotARepository);
    }

    let Some(expected) = expected_remote.filter(|u| !u.trim().is_empty()) else {
        let Some(expected_root) = expected_initial_commit.filter(|c| !c.trim().is_empty()) else {
            return Ok(RelocateCheck::CannotVerify);
        };
        // 付け替え先に保存が無い、または登録時の初期 commit が履歴に無ければ別のプロジェクト
        let roots = root_commits(runner, new_path)?;
        return Ok(if roots.iter().any(|r| r == expected_root.trim()) {
            RelocateCheck::Same
        } else {
            RelocateCheck::DifferentRepository
        });
    };
    let remote = runner.run(new_path, &["remote", "get-url", "origin"])?;
    if remote.code != 0 {
        return Ok(RelocateCheck::DifferentRepository);
    }
    let actual = String::from_utf8_lossy(&remote.stdout).trim().to_string();
    if normalize_remote_url(&actual) == normalize_remote_url(expected) {
        Ok(RelocateCheck::Same)
    } else {
        Ok(RelocateCheck::DifferentRepository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_with_different_forms_compare_equal() {
        let forms = [
            "https://github.com/Owner/Repo.git",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo/",
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
            "https://user:token@github.com/owner/repo.git",
        ];
        let first = normalize_remote_url(forms[0]);
        assert_eq!(first, "github.com/owner/repo");
        for f in forms {
            assert_eq!(normalize_remote_url(f), first, "{f}");
        }
        assert_ne!(
            normalize_remote_url("https://github.com/owner/other.git"),
            first
        );
    }

    #[test]
    fn root_commits_are_the_lines_without_parents() {
        let out = "c3 c2\nc2 c1\nc1\nm1 c3 x9\nx9\n";
        assert_eq!(parse_root_commits(out), ["c1", "x9"]);
        assert!(parse_root_commits("").is_empty());
        assert!(parse_root_commits("a b\n").is_empty());
    }

    #[test]
    fn local_paths_are_kept_comparable() {
        assert_eq!(
            normalize_remote_url("/tmp/remote.git"),
            normalize_remote_url("/tmp/remote")
        );
    }
}
