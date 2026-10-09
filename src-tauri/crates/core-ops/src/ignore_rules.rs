// 保存しないファイル（設計書 4.8、3.2 IgnoreRuleRow、S6）。
//
// - 対象はプロジェクト直下の `.gitignore` だけ。一覧・追加・削除の範囲で作業フォルダとインデックスには触れない
// - 追加・削除は `.gitignore` を書き換えるため、変更の前に復元点を作る（不変条件 3）。保存はしない
//   （`.gitignore` の変更は通常の未保存の変更として扱われる）
// - 追跡中の該当ファイルを保存対象から外すのは `untrack_and_save`（復元点 → `rm --cached -r` → 保存）。
//   作業フォルダのファイルは消さない（不変条件 6）
// - 「今後の変更だけ無視する」（skip-worktree）はスライス B。ここには含めない

use crate::models::{OpsError, RestorePointInfo, SaveOutcome};
use crate::operations::{conflicts, current_branch, stage_and_commit};
use crate::pc_name::Meta;
use crate::restore_file::normalize_project_path;
use crate::size_check::{append_lines, escape_path, ignore_pattern, untrack_paths};
use core_git::GitRunner;
use serde::{Deserialize, Serialize};
use std::path::Path;
use time::OffsetDateTime;

/// 保存しないファイルの追加の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IgnoreKind {
    /// 拡張子（`*.ext`。どの階層のファイルにも当たる）
    Extension,
    /// フォルダ（`name/`。プロジェクト内のパスで指定でき、その階層のフォルダに当たる）
    Folder,
    /// 個別のファイル（`/path`。ルート直下からの完全一致）
    File,
}

/// `.gitignore` の 1 行（コメントと空行を除く）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoreRule {
    /// 行の内容（手書きの行も含む）
    pub pattern: String,
    /// このパターンに当たる、追跡中（保存対象になっている）ファイルの件数。
    /// この画面で解釈できない書き方（否定 `!`、`**`、文字クラス `[...]`）の行は None
    pub tracked_count: Option<u32>,
}

/// `.gitignore` の内容から、パターンの行を取り出す（純関数）。コメントと空行を除く。
pub fn gitignore_patterns(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// 種類と入力から、`.gitignore` に書くパターンを作る（純関数）。
///
/// - 拡張子: 先頭の `.` は外す。空、または `/ \ * ? [ ]` や空白・制御文字を含むものは拒否する
/// - フォルダ・ファイル: プロジェクト内の相対パスとして検査し、git の特別扱いの文字をエスケープする
pub fn build_pattern(kind: IgnoreKind, value: &str) -> Result<String, OpsError> {
    match kind {
        IgnoreKind::Extension => {
            let ext = value.trim().trim_start_matches('.');
            let usable = !ext.is_empty()
                && !ext.chars().any(|c| {
                    c.is_control()
                        || c.is_whitespace()
                        || matches!(c, '/' | '\\' | '*' | '?' | '[' | ']')
                });
            if !usable {
                return Err(OpsError::InvalidInput(format!(
                    "extension '{value}' is not usable"
                )));
            }
            Ok(format!("*.{ext}"))
        }
        IgnoreKind::Folder => {
            let rel = normalize_project_path(value)?;
            Ok(format!("{}/", escape_path(&rel)))
        }
        IgnoreKind::File => {
            let rel = normalize_project_path(value)?;
            Ok(ignore_pattern(&rel))
        }
    }
}

/// パターン 1 行が、パス（`/` 区切り、ファイル）に当たるか（純関数）。
///
/// 解釈するのは、この画面が作る書き方（`*.ext`、`name/`、`/path`、`a/b/`）と、それに準ずる単純な glob
/// （`*`、`?`、`\` によるエスケープ）だけ。扱えない書き方は `None` を返す。
/// 大文字・小文字は区別する（git の `core.ignorecase` は考慮しない）。
pub fn pattern_matches(pattern: &str, path: &str) -> Option<bool> {
    if !is_supported(pattern) {
        return None;
    }
    let dir_only = pattern.ends_with('/');
    let body = pattern.strip_suffix('/').unwrap_or(pattern);
    // 途中または先頭に `/` があれば、プロジェクト直下からの指定（アンカー）
    let anchored = body.contains('/');
    let body = body.strip_prefix('/').unwrap_or(body);
    let pat: Vec<Vec<char>> = body.split('/').map(|s| s.chars().collect()).collect();
    let segs: Vec<&str> = path.split('/').collect();

    let hit = if anchored {
        // 先頭から順に一致させる。パターンがパスの途中で終われば、それはフォルダなので中のファイルも対象。
        // パターンがパスの全体に一致するときは、フォルダを指す書き方（末尾 `/`）でなければ対象
        pat.len() <= segs.len()
            && pat.iter().zip(&segs).all(|(p, s)| glob_match(p, s))
            && (pat.len() < segs.len() || !dir_only)
    } else {
        // 名前だけの書き方は、どの階層のものにも当たる（途中のフォルダ名に当たれば中のファイルも対象）
        let last = segs.len() - 1;
        segs.iter()
            .enumerate()
            .any(|(i, s)| glob_match(&pat[0], s) && (i < last || !dir_only))
    };
    Some(hit)
}

/// この画面で解釈できる書き方か（否定・`**`・文字クラスは解釈しない）
pub fn is_supported(pattern: &str) -> bool {
    !pattern.starts_with('!') && !pattern.contains("**") && !has_char_class(pattern)
}

/// 文字クラス `[...]` を含むか（`\[` のエスケープは含まない）
fn has_char_class(pattern: &str) -> bool {
    let mut escaped = false;
    for c in pattern.chars() {
        if escaped {
            escaped = false;
        } else {
            match c {
                '\\' => escaped = true,
                '[' => return true,
                _ => {}
            }
        }
    }
    false
}

/// 1 階層の名前に対する glob の一致（`*`、`?`、`\` によるエスケープ）
fn glob_match(pat: &[char], text: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    glob_match_chars(pat, &text)
}

fn glob_match_chars(pat: &[char], text: &[char]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some('*') => (0..=text.len()).any(|i| glob_match_chars(&pat[1..], &text[i..])),
        Some('?') => !text.is_empty() && glob_match_chars(&pat[1..], &text[1..]),
        Some('\\') if pat.len() >= 2 => {
            !text.is_empty() && text[0] == pat[1] && glob_match_chars(&pat[2..], &text[1..])
        }
        Some(c) => !text.is_empty() && text[0] == *c && glob_match_chars(&pat[1..], &text[1..]),
    }
}

/// 追跡中（インデックスにあり）で、除外設定に当たるファイルのパス（`/` 区切り）を返す。
/// 読み取りのみ。`ls-files -ci --exclude-standard -z` の結果。
pub(crate) fn tracked_ignored_files(
    runner: &GitRunner,
    repo: &Path,
) -> Result<Vec<String>, OpsError> {
    let out = runner.run_ok(repo, &["ls-files", "-ci", "--exclude-standard", "-z"])?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// `.gitignore` の内容を読む。ファイルが無ければ空。UTF-8 でなければ書き換えないよう拒否する。
fn read_gitignore(repo: &Path) -> Result<String, OpsError> {
    match std::fs::read(repo.join(".gitignore")) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| OpsError::Unexpected(".gitignore is not valid UTF-8".to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// 変更の前の復元点を作る（不変条件 3）。`.gitignore` は作業フォルダのファイルなので、
/// 保存するものが無くても作る（復元点は HEAD が無くても作れる）。
fn restore_point_before(
    runner: &GitRunner,
    repo: &Path,
    meta: Meta,
    operation: &str,
    now: OffsetDateTime,
) -> Result<RestorePointInfo, OpsError> {
    let branch = current_branch(runner, repo)?;
    let point = meta.restore_point(runner, repo, &branch, operation, now)?;
    Ok(RestorePointInfo {
        snapshot_ref: point.snapshot.as_ref().map(|s| s.ref_name.clone()),
        backup_ref: point.backup_ref,
    })
}

/// マージ競合が残っているときは、この操作を拒否する（保存と同じ扱い）
fn refuse_during_conflict(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    let current = conflicts(runner, repo)?;
    if current.is_empty() {
        Ok(())
    } else {
        Err(OpsError::Conflict(current))
    }
}

/// 保存しないファイルのパターン一覧を、件数つきで返す（読み取りのみ）。
pub(crate) fn list_ignore_rules(
    runner: &GitRunner,
    repo: &Path,
) -> Result<Vec<IgnoreRule>, OpsError> {
    let patterns = gitignore_patterns(&read_gitignore(repo)?);
    let tracked = tracked_ignored_files(runner, repo)?;
    Ok(patterns
        .into_iter()
        .map(|pattern| {
            let tracked_count = is_supported(&pattern).then(|| {
                let n = tracked
                    .iter()
                    .filter(|path| pattern_matches(&pattern, path) == Some(true))
                    .count();
                u32::try_from(n).unwrap_or(u32::MAX)
            });
            IgnoreRule {
                pattern,
                tracked_count,
            }
        })
        .collect())
}

/// パターンを `.gitignore` に追加する。戻り値は書いたパターン。
/// 同じ行がすでにあれば何も書かず（復元点も作らず）、そのパターンを返す。保存はしない。
pub(crate) fn add_ignore_pattern(
    runner: &GitRunner,
    repo: &Path,
    kind: IgnoreKind,
    value: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<String, OpsError> {
    refuse_during_conflict(runner, repo)?;
    let pattern = build_pattern(kind, value)?;
    if gitignore_patterns(&read_gitignore(repo)?).contains(&pattern) {
        return Ok(pattern);
    }
    restore_point_before(runner, repo, meta, "ignore-add", now)?;
    append_lines(repo, std::slice::from_ref(&pattern))?;
    Ok(pattern)
}

/// `.gitignore` から、完全一致する 1 行だけを消す（同じ内容の行が複数あれば最初の 1 行）。
/// 保存はしない。見つからなければ `InvalidInput`。
pub(crate) fn remove_ignore_pattern(
    runner: &GitRunner,
    repo: &Path,
    pattern: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<(), OpsError> {
    refuse_during_conflict(runner, repo)?;
    if pattern.trim().is_empty() || pattern.starts_with('#') {
        return Err(OpsError::InvalidInput(
            "pattern is not a rule line".to_string(),
        ));
    }
    let text = read_gitignore(repo)?;
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let index = lines
        .iter()
        .position(|line| line.trim_end_matches(['\r', '\n']) == pattern)
        .ok_or_else(|| OpsError::InvalidInput("pattern is not in .gitignore".to_string()))?;
    lines.remove(index);
    let next: String = lines.concat();

    restore_point_before(runner, repo, meta, "ignore-remove", now)?;

    // 一時ファイルへ書いてから差し替える（途中で失敗しても元の内容が残る）
    let path = repo.join(".gitignore");
    let tmp = repo.join(format!(
        ".gitignore.hikae-tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let written = std::fs::write(&tmp, next.as_bytes()).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// 追跡中の該当ファイルを保存対象から外して保存する（設計書 4.8 の 2）。
///
/// 1. `paths` は `tracked_ignored_files` に出るファイルだけを受け付ける（それ以外は `InvalidInput`）
/// 2. 復元点を作る（操作名 `untrack`）
/// 3. `rm --cached -r` で追跡から外す（作業フォルダのファイルは残る）
/// 4. `add -A` → commit（`.gitignore` の変更もこの保存に入る）
pub(crate) fn untrack_and_save(
    runner: &GitRunner,
    repo: &Path,
    paths: &[String],
    memo: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<SaveOutcome, OpsError> {
    refuse_during_conflict(runner, repo)?;
    if paths.is_empty() {
        return Err(OpsError::InvalidInput("no files to untrack".to_string()));
    }
    let ignored = tracked_ignored_files(runner, repo)?;
    let mut selected: Vec<String> = Vec::new();
    for raw in paths {
        let rel = normalize_project_path(raw)?;
        if !ignored.contains(&rel) {
            return Err(OpsError::InvalidInput(format!(
                "'{rel}' is not a tracked file that is ignored"
            )));
        }
        if !selected.contains(&rel) {
            selected.push(rel);
        }
    }

    let restore_point = restore_point_before(runner, repo, meta, "untrack", now)?;
    untrack_paths(runner, repo, &selected)?;

    // 保存する内容が無ければ空の保存は作らない
    match stage_and_commit(runner, repo, memo, meta)? {
        Some(commit) => Ok(SaveOutcome::Saved {
            commit,
            restore_point,
        }),
        None => Ok(SaveOutcome::NothingToSave),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_skip_comments_and_blank_lines() {
        let text = "# comment\n\n*.log\r\n  \n/a.txt\n#x\n";
        assert_eq!(gitignore_patterns(text), vec!["*.log", "/a.txt"]);
    }

    #[test]
    fn builds_patterns_for_each_kind() {
        assert_eq!(
            build_pattern(IgnoreKind::Extension, ".psd").unwrap(),
            "*.psd"
        );
        assert_eq!(
            build_pattern(IgnoreKind::Extension, " tar.gz ").unwrap(),
            "*.tar.gz"
        );
        assert_eq!(
            build_pattern(IgnoreKind::Folder, "node_modules").unwrap(),
            "node_modules/"
        );
        assert_eq!(build_pattern(IgnoreKind::Folder, "a\\b/").unwrap(), "a/b/");
        assert_eq!(
            build_pattern(IgnoreKind::File, "docs/動画 1.mp4").unwrap(),
            "/docs/動画 1.mp4"
        );
        assert_eq!(
            build_pattern(IgnoreKind::File, "#x*[1].bin").unwrap(),
            "/#x\\*\\[1\\].bin"
        );
    }

    #[test]
    fn rejects_unusable_input() {
        assert!(build_pattern(IgnoreKind::Extension, "").is_err());
        assert!(build_pattern(IgnoreKind::Extension, ".").is_err());
        assert!(build_pattern(IgnoreKind::Extension, "a/b").is_err());
        assert!(build_pattern(IgnoreKind::Extension, "*").is_err());
        assert!(build_pattern(IgnoreKind::Extension, "p s").is_err());
        assert!(build_pattern(IgnoreKind::Folder, "../x").is_err());
        assert!(build_pattern(IgnoreKind::Folder, "/abs").is_err());
        assert!(build_pattern(IgnoreKind::File, ".git/config").is_err());
        assert!(build_pattern(IgnoreKind::File, "").is_err());
    }

    #[test]
    fn extension_pattern_matches_at_any_depth() {
        assert_eq!(pattern_matches("*.psd", "a/b/c.psd"), Some(true));
        assert_eq!(pattern_matches("*.psd", "c.psd"), Some(true));
        assert_eq!(pattern_matches("*.psd", "c.psd.txt"), Some(false));
        assert_eq!(pattern_matches("*.psd", "psd/c.txt"), Some(false));
        // 名前がフォルダでも、その中のファイルは対象
        assert_eq!(pattern_matches("*.psd", "dir.psd/x.txt"), Some(true));
    }

    #[test]
    fn folder_pattern_matches_directories_only() {
        assert_eq!(
            pattern_matches("node_modules/", "a/node_modules/x.js"),
            Some(true)
        );
        assert_eq!(
            pattern_matches("node_modules/", "node_modules/x.js"),
            Some(true)
        );
        assert_eq!(
            pattern_matches("node_modules/", "node_modules"),
            Some(false)
        );
        assert_eq!(
            pattern_matches("node_modules/", "node_modules_x/y"),
            Some(false)
        );
    }

    #[test]
    fn anchored_patterns_match_from_the_root() {
        assert_eq!(pattern_matches("/a.txt", "a.txt"), Some(true));
        assert_eq!(pattern_matches("/a.txt", "x/a.txt"), Some(false));
        assert_eq!(pattern_matches("/a/b/", "a/b/c.txt"), Some(true));
        assert_eq!(pattern_matches("/a/b/", "x/a/b/c.txt"), Some(false));
        assert_eq!(pattern_matches("a/b/", "a/b/c"), Some(true));
        assert_eq!(pattern_matches("a/b/", "x/a/b/c"), Some(false));
        assert_eq!(pattern_matches("/a", "a/b.txt"), Some(true));
        assert_eq!(pattern_matches("/a", "a"), Some(true));
    }

    #[test]
    fn escaped_patterns_match_literal_names() {
        let pattern = ignore_pattern("#x*[1]?.bin");
        assert_eq!(pattern_matches(&pattern, "#x*[1]?.bin"), Some(true));
        assert_eq!(pattern_matches(&pattern, "#xa[1]b.bin"), Some(false));
        let spaced = ignore_pattern("end  ");
        assert_eq!(pattern_matches(&spaced, "end  "), Some(true));
    }

    #[test]
    fn unsupported_syntax_is_unknown() {
        assert_eq!(pattern_matches("!keep.psd", "keep.psd"), None);
        assert_eq!(pattern_matches("a/**/b", "a/x/b"), None);
        assert_eq!(pattern_matches("[ab].txt", "a.txt"), None);
    }

    #[test]
    fn wildcard_inside_name() {
        assert_eq!(pattern_matches("~$*", "~$report.docx"), Some(true));
        assert_eq!(pattern_matches("~$*", "report.docx"), Some(false));
        assert_eq!(pattern_matches("a?c", "abc"), Some(true));
    }
}
