// 新規（未追跡）ファイルの差分表示（設計書 3.3 S2、4.1）。
//
// `git diff` は、まだ保存もインデックスへの登録もされていないファイルを出力しない。そのため
// 「いま」との比較で対象が新規ファイルのときは、作業フォルダのファイルを直接読み、全行を
// 「追加」の差分行として返す。
// - 読むのはプロジェクト内の通常のファイルだけ。シンボリックリンクはたどらず、本文を出さない
// - NUL を含む（バイナリ）か、大きすぎる（`NEW_FILE_DIFF_MAX_BYTES` 超）場合は、本文を返さず
//   サイズと更新日時だけを返す
// - 作業フォルダ・インデックスは変更しない（読み取りのみ）

use crate::models::{DiffLine, DiffLineKind, OpsError};
use crate::open_path::{resolve_entry_in_project, OpenPathError};
use crate::operations::diff_with;
use crate::restore_file::{normalize_project_path, resolve_commit, size_at};
use core_git::GitRunner;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// 新規ファイルの本文を差分として出す上限（バイト）。超えるものは「大きすぎて省略」にする
pub const NEW_FILE_DIFF_MAX_BYTES: u64 = 1024 * 1024;

/// 差分の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiffOutcome {
    /// 通常の差分（`git diff` の出力）
    Lines(Vec<DiffLine>),
    /// 新規ファイル。全行が追加の差分行（行番号つき）
    NewFileLines(Vec<DiffLine>),
    /// 内容が空の新規ファイル
    NewFileEmpty,
    /// 本文を出せない新規ファイル（バイナリ、またはシンボリックリンク）
    NewFileBinary {
        size: u64,
        modified_unix: Option<i64>,
    },
    /// 大きすぎて本文を省略した新規ファイル
    NewFileTooLarge {
        size: u64,
        modified_unix: Option<i64>,
    },
}

/// `from` と作業フォルダ（`to` が `current`）の差分。`path` が新規ファイルなら作業フォルダを直接読む。
/// それ以外は `diff_with` と同じ。
pub(crate) fn diff_file(
    runner: &GitRunner,
    repo: &Path,
    from: &str,
    to: &str,
    path: Option<&str>,
) -> Result<FileDiffOutcome, OpsError> {
    if let (true, Some(raw)) = (to == "current", path) {
        // プロジェクト外・`..`・`.git` 配下などは、ここで拒否する
        let rel = normalize_project_path(raw)?;
        if is_new_file(runner, repo, from, &rel)? {
            return read_new_file(repo, &rel);
        }
    }
    Ok(FileDiffOutcome::Lines(diff_with(
        runner, repo, from, to, path,
    )?))
}

/// `rel` が「`from` にも無く、インデックスにも無い、作業フォルダにある」ファイルか。
/// 判定できない（解決できない `from` など）ときは false を返し、通常の差分に任せる。
fn is_new_file(runner: &GitRunner, repo: &Path, from: &str, rel: &str) -> Result<bool, OpsError> {
    // インデックスにあるもの（追跡中・登録済み）は git の差分に任せる
    let spec = format!(":(literal){rel}");
    let tracked = runner.run_ok(repo, &["ls-files", "-z", "--cached", "--", &spec])?;
    if !tracked.stdout.is_empty() {
        return Ok(false);
    }
    // 比べる時点に存在するファイルは、git が「削除された」差分を出す
    if from != "current" {
        let Ok(oid) = resolve_commit(runner, repo, from) else {
            return Ok(false);
        };
        if size_at(runner, repo, &oid, rel)?.is_some() {
            return Ok(false);
        }
    }
    // 作業フォルダに実体（リンクを含む）があること
    let Ok(abs) = resolve_entry_in_project(repo, rel) else {
        return Ok(false);
    };
    Ok(matches!(
        std::fs::symlink_metadata(&abs),
        Ok(meta) if !meta.is_dir()
    ))
}

/// 作業フォルダの新規ファイルを読んで、全行が追加の差分にする。
fn read_new_file(repo: &Path, rel: &str) -> Result<FileDiffOutcome, OpsError> {
    let abs = resolve_entry_in_project(repo, rel).map_err(open_path_to_ops)?;
    let meta = std::fs::symlink_metadata(&abs)?;
    let modified_unix = modified_unix(&meta);

    // リンクはたどらない。リンク先の本文は出さず、情報だけにする
    if !meta.is_file() {
        return Ok(FileDiffOutcome::NewFileBinary {
            size: meta.len(),
            modified_unix,
        });
    }
    if meta.len() > NEW_FILE_DIFF_MAX_BYTES {
        return Ok(FileDiffOutcome::NewFileTooLarge {
            size: meta.len(),
            modified_unix,
        });
    }

    // 判定から読み込みまでの間に大きくなった場合に備え、上限 + 1 バイトまでしか読まない
    let mut bytes = Vec::new();
    std::fs::File::open(&abs)?
        .take(NEW_FILE_DIFF_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let size = bytes.len() as u64;
    if size > NEW_FILE_DIFF_MAX_BYTES {
        return Ok(FileDiffOutcome::NewFileTooLarge {
            size,
            modified_unix,
        });
    }
    if bytes.contains(&0) {
        return Ok(FileDiffOutcome::NewFileBinary {
            size,
            modified_unix,
        });
    }
    if bytes.is_empty() {
        return Ok(FileDiffOutcome::NewFileEmpty);
    }
    Ok(FileDiffOutcome::NewFileLines(all_added_lines(
        &String::from_utf8_lossy(&bytes),
    )))
}

/// 本文を行に分け、全行を追加行（新しい側の行番号は 1 から）にする。
/// 末尾の改行は行として数えず、行末の `\r` は除く。
fn all_added_lines(text: &str) -> Vec<DiffLine> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .enumerate()
        .map(|(i, line)| DiffLine {
            kind: DiffLineKind::Added,
            line_number_old: None,
            line_number_new: Some(u32::try_from(i + 1).unwrap_or(u32::MAX)),
            content: line.strip_suffix('\r').unwrap_or(line).to_string(),
        })
        .collect()
}

fn modified_unix(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

fn open_path_to_ops(e: OpenPathError) -> OpsError {
    match e {
        OpenPathError::Io(io) | OpenPathError::RootUnavailable(io) => OpsError::from(io),
        other => OpsError::InvalidInput(format!("path is not allowed: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_numbered_from_one_and_trailing_newline_is_not_a_line() {
        let lines = all_added_lines("a\nb\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_number_new, Some(1));
        assert_eq!(lines[1].content, "b");
        assert!(lines.iter().all(|l| l.kind == DiffLineKind::Added));
        assert!(lines.iter().all(|l| l.line_number_old.is_none()));
    }

    #[test]
    fn last_line_without_newline_and_crlf_are_handled() {
        let lines = all_added_lines("x\r\ny");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, "x");
        assert_eq!(lines[1].content, "y");
    }

    #[test]
    fn blank_lines_are_kept() {
        let lines = all_added_lines("\n\n");
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.content.is_empty()));
    }
}
