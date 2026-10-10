// 保存メモの提案に渡すテキストの差分（設計書 10.4）。読み取りのみで、作業フォルダ・インデックス・refs に書かない。
//
// - 比べる相手は HEAD（この PC の最新の保存）。HEAD が無い（最初の保存の前）ときは、作業フォルダの内容を全部追加として扱う
// - テキストとして読めないもの（バイナリ）・上限を超えるもの・新規のバイナリは None（本文は渡さない）
// - 行数は全体を数え、差分の本文は `MEMO_DIFF_MAX_LINES` 行までにする（残りは件数だけ）
// - 文書（docx・xlsx）の差分は app 層が `core-preview` で行う。ここはテキストだけを扱う

use crate::blob::{read_text, TextFile, WORK_REV};
use crate::models::{DiffLineKind, OpsError};
use crate::new_file_diff::{diff_file, FileDiffOutcome};
use crate::point::resolve_point;
use core_git::GitRunner;
use std::path::Path;

/// 差分を読む元ファイルの上限（バイト）。これを超えるファイルは本文を渡さない
pub const MEMO_TEXT_MAX_BYTES: u64 = 256 * 1024;
/// 本文として渡す差分の行数の上限
pub const MEMO_DIFF_MAX_LINES: usize = 400;

/// 保存メモに渡すテキストの差分
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoTextDiff {
    /// 追加された行の数（全体）
    pub added: usize,
    /// 削除された行の数（全体）
    pub removed: usize,
    /// 追加・削除の行。`+ ` / `- ` の接頭辞を付け、`MEMO_DIFF_MAX_LINES` 行まで
    pub text: String,
}

/// テキストの差分を取る（読み取りのみ）。`path` はプロジェクト相対パス。
pub(crate) fn memo_text_diff(
    runner: &GitRunner,
    repo: &Path,
    path: &str,
    max_bytes: u64,
) -> Result<Option<MemoTextDiff>, OpsError> {
    // 作業フォルダ側の内容。無い（削除された）ときは None、テキストでないときは本文を渡さない
    let work = match read_text(runner, repo, WORK_REV, path, max_bytes) {
        Ok(TextFile::Text(text)) => Some(text),
        Ok(TextFile::Binary | TextFile::TooLarge) => return Ok(None),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let head = match resolve_point(runner, repo, "this") {
        Ok(oid) => Some(oid),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let Some(head) = head else {
        // 最初の保存の前。作業フォルダの内容を全部追加として扱う
        return Ok(work.map(|text| all_added(&text)));
    };
    let outcome = diff_file(runner, repo, &head, WORK_REV, Some(path))?;
    Ok(match outcome {
        FileDiffOutcome::Lines(lines) | FileDiffOutcome::NewFileLines(lines) => {
            // 作業フォルダの版が無いのに差分が取れた（削除）ときは、そのまま行を使う
            Some(collect(lines.iter().map(|l| (l.kind, l.content.as_str()))))
        }
        FileDiffOutcome::NewFileEmpty => Some(MemoTextDiff {
            added: 0,
            removed: 0,
            text: String::new(),
        }),
        FileDiffOutcome::NewFileBinary { .. } | FileDiffOutcome::NewFileTooLarge { .. } => None,
    })
}

/// 本文の全行を追加として数える
fn all_added(text: &str) -> MemoTextDiff {
    collect(text.lines().map(|line| (DiffLineKind::Added, line)))
}

/// 行の列から件数と本文（上限まで）を作る。文脈行は本文に入れない
fn collect<'a>(lines: impl Iterator<Item = (DiffLineKind, &'a str)>) -> MemoTextDiff {
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut text = String::new();
    let mut shown = 0usize;
    for (kind, content) in lines {
        let sign = match kind {
            DiffLineKind::Added => {
                added += 1;
                '+'
            }
            DiffLineKind::Removed => {
                removed += 1;
                '-'
            }
            DiffLineKind::Context => continue,
        };
        if shown < MEMO_DIFF_MAX_LINES {
            text.push(sign);
            text.push(' ');
            text.push_str(content);
            text.push('\n');
            shown += 1;
        }
    }
    MemoTextDiff {
        added,
        removed,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cover_all_lines_but_the_text_is_capped() {
        let lines: Vec<String> = (0..MEMO_DIFF_MAX_LINES + 10)
            .map(|i| format!("行{i}"))
            .collect();
        let joined = lines.join("\n");
        let diff = all_added(&joined);
        assert_eq!(diff.added, MEMO_DIFF_MAX_LINES + 10);
        assert_eq!(diff.removed, 0);
        assert_eq!(diff.text.lines().count(), MEMO_DIFF_MAX_LINES);
        assert!(diff.text.starts_with("+ 行0\n"));
    }

    #[test]
    fn context_lines_are_not_counted_or_shown() {
        let diff = collect(
            [
                (DiffLineKind::Context, "同じ"),
                (DiffLineKind::Removed, "旧"),
                (DiffLineKind::Added, "新"),
            ]
            .into_iter(),
        );
        assert_eq!((diff.added, diff.removed), (1, 1));
        assert_eq!(diff.text, "- 旧\n+ 新\n");
    }
}
