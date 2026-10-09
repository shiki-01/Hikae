// OS・Office の一時ファイルを、リポジトリ単位の除外設定（`.git/info/exclude`）へ入れる
// （設計書 13.1「Office の一時ファイルの混入」、8.3）。
//
// - 書き込む先は `.git/info/exclude` だけ。作業フォルダのファイルにも、利用者の `.gitignore` にも、
//   インデックスにも触れない（`.gitignore` と違い、他の PC やクラウドへは伝わらない）
// - 既存の内容は壊さない。アプリが管理する部分はマーカーのコメントで囲み、そこだけを更新する。
//   マーカーの外に同じ行がすでにあれば、重複して書かない
// - `*.tmp` のような広いパターンは入れない（利用者のファイルを誤って除外しないため）
// - 除外設定の変更の前に復元点を作る（不変条件 3）。保存するものが何も無い（保存が 1 つも無く、
//   作業フォルダにもファイルが無い）ときは、復元点を作らない
// - 書き込みは一時ファイルへ書いてから差し替える（途中で失敗しても元の内容が残る）

use crate::models::OpsError;
use crate::operations::{current_branch, read_status};
use crate::pc_name::Meta;
use core_git::GitRunner;
use std::path::Path;
use time::OffsetDateTime;

/// アプリが管理する部分の開始行
pub const EXCLUDE_BLOCK_BEGIN: &str = "# >>> Hikae managed block (edit outside this block) >>>";
/// アプリが管理する部分の終了行
pub const EXCLUDE_BLOCK_END: &str = "# <<< Hikae managed block <<<";

/// 初期の除外パターン。Office の所有者ファイル・ロックファイル、Word の一時ファイル、
/// macOS・Windows のフォルダ情報ファイル。
pub const DEFAULT_EXCLUDE_PATTERNS: [&str; 6] = [
    "~$*",
    ".~lock.*#",
    "~WRL*.tmp",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
];

/// 復元点の操作名（`refs/hikae/backup/<操作名>/`）
const OPERATION: &str = "set-excludes";

/// 除外設定を更新した結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeOutcome {
    /// 書き込んだ（新しく入れた、または管理する部分を更新した）
    Written,
    /// 何もしなかった（すでに最新、`only_if_missing` でマーカーがある、対象外のリポジトリ、
    /// 壊れた管理部分や UTF-8 でない内容は触らない）
    Unchanged,
}

/// 管理する部分を適用した新しい内容を作る（純関数）。変更が無ければ `None`。
///
/// - 管理する部分が壊れている（開始行だけがある・終了行が先にある）ときは何もしない
/// - `only_if_missing` のときは、開始行がすでにあれば何もしない
pub fn apply_block(existing: &str, only_if_missing: bool) -> Option<String> {
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    let begin = lines.iter().position(|l| l.trim() == EXCLUDE_BLOCK_BEGIN);
    let end = lines.iter().position(|l| l.trim() == EXCLUDE_BLOCK_END);
    let (before, after): (Vec<&str>, Vec<&str>) = match (begin, end) {
        (Some(_), _) if only_if_missing => return None,
        (Some(b), Some(e)) if b < e => (lines[..b].to_vec(), lines[e + 1..].to_vec()),
        (Some(_), _) | (None, Some(_)) => return None,
        (None, None) => (lines.clone(), Vec::new()),
    };

    // マーカーの外にすでにある行は、重複して書かない
    let outside: std::collections::HashSet<&str> = before
        .iter()
        .chain(after.iter())
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mut block = String::new();
    block.push_str(EXCLUDE_BLOCK_BEGIN);
    block.push_str(eol);
    for pattern in DEFAULT_EXCLUDE_PATTERNS {
        if !outside.contains(pattern) {
            block.push_str(pattern);
            block.push_str(eol);
        }
    }
    block.push_str(EXCLUDE_BLOCK_END);
    block.push_str(eol);

    let mut next = String::new();
    for line in &before {
        next.push_str(line);
    }
    // 末尾に改行が無い既存の内容の後ろへ足すときは、先に改行する
    if begin.is_none() && !next.is_empty() && !next.ends_with('\n') {
        next.push_str(eol);
    }
    next.push_str(&block);
    for line in &after {
        next.push_str(line);
    }
    if next == existing {
        None
    } else {
        Some(next)
    }
}

/// 除外設定を適用する。`only_if_missing` が真なら、管理する部分がまだ無いときだけ書く
/// （起動時に既存のプロジェクトへ 1 回だけ適用する場合）。
pub(crate) fn apply_default_excludes(
    runner: &GitRunner,
    repo: &Path,
    only_if_missing: bool,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<ExcludeOutcome, OpsError> {
    // 通常のリポジトリ（`.git` がフォルダ）だけを対象にする
    let git_dir = repo.join(".git");
    if !std::fs::symlink_metadata(&git_dir).is_ok_and(|m| m.is_dir()) {
        return Ok(ExcludeOutcome::Unchanged);
    }
    let path = git_dir.join("info").join("exclude");
    let existing = match std::fs::read(&path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            // UTF-8 でない内容を、読み替えて壊さない
            Err(_) => return Ok(ExcludeOutcome::Unchanged),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let Some(next) = apply_block(&existing, only_if_missing) else {
        return Ok(ExcludeOutcome::Unchanged);
    };

    // 復元点（保存するものが何も無いときは作らない）
    let has_head = runner
        .run(repo, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .is_ok_and(|o| o.code == 0);
    if has_head || !read_status(runner, repo)?.entries.is_empty() {
        let branch = current_branch(runner, repo)?;
        meta.restore_point(runner, repo, &branch, OPERATION, now)?;
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_file_name(format!(
        "exclude.hikae-tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let written = std::fs::write(&tmp, next.as_bytes()).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(ExcludeOutcome::Written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gets_the_block() {
        let next = apply_block("", false).expect("changed");
        assert!(next.starts_with(EXCLUDE_BLOCK_BEGIN));
        assert!(next.contains("~$*\n"));
        assert!(next.contains(".~lock.*#\n"));
        assert!(next.ends_with(&format!("{EXCLUDE_BLOCK_END}\n")));
        // 広いパターンは入れない
        assert!(!next.lines().any(|l| l == "*.tmp"));
    }

    #[test]
    fn existing_lines_are_kept_and_not_duplicated() {
        let existing = "# my rules\n*.log\nThumbs.db";
        let next = apply_block(existing, false).expect("changed");
        assert!(next.starts_with("# my rules\n*.log\nThumbs.db\n"));
        // マーカーの外にある行は、ブロックの中に重ねて書かない
        assert_eq!(next.lines().filter(|l| *l == "Thumbs.db").count(), 1);
        assert!(next.contains("desktop.ini"));
    }

    #[test]
    fn applying_twice_is_stable_and_updates_only_the_block() {
        let first = apply_block("*.log\n", false).expect("changed");
        assert_eq!(apply_block(&first, false), None);
        // 管理する部分の中を書き換えられていたら、元に戻す。外の内容は保つ
        let tampered = first
            .replace("desktop.ini\n", "")
            .replace("*.log\n", "*.log\n*.bak\n");
        let fixed = apply_block(&tampered, false).expect("changed");
        assert!(fixed.contains("desktop.ini"));
        assert!(fixed.contains("*.bak\n"));
        assert_eq!(fixed.matches(EXCLUDE_BLOCK_BEGIN).count(), 1);
    }

    #[test]
    fn only_if_missing_leaves_an_existing_block_alone() {
        let first = apply_block("", false).expect("changed");
        let edited = first.replace("desktop.ini\n", "");
        assert_eq!(apply_block(&edited, true), None);
    }

    #[test]
    fn broken_markers_are_not_touched() {
        let only_begin = format!("{EXCLUDE_BLOCK_BEGIN}\n~$*\n");
        assert_eq!(apply_block(&only_begin, false), None);
        let reversed = format!("{EXCLUDE_BLOCK_END}\nx\n{EXCLUDE_BLOCK_BEGIN}\n");
        assert_eq!(apply_block(&reversed, false), None);
    }

    #[test]
    fn crlf_files_keep_their_line_endings() {
        let next = apply_block("*.log\r\n", false).expect("changed");
        assert!(next.contains("~$*\r\n"));
        assert!(!next.replace("\r\n", "").contains('\n'));
    }
}
