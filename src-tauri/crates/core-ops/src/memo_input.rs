// 保存メモの提案に渡すテキストの差分（設計書 10.4）。読み取りのみで、作業フォルダ・インデックス・refs に書かない。
//
// - 比べる相手は HEAD（この PC の最新の保存）。HEAD が無い（最初の保存の前）ときは、作業フォルダの内容を全部追加として扱う
// - 文字コードは UTF-8（先頭の BOM を除く）と Shift_JIS を読む。どちらでもないもの・NUL を含むもの（バイナリ）・
//   上限を超えるものは None（本文は渡さない）
// - 追跡中のファイルの差分は `git diff` の出力をバイト列のまま読む（`diff_with` は文字列へ変換するため、Shift_JIS が崩れる）。
//   UI の差分表示（`diff_file`）は変えない
// - 行数は全体を数え、差分の本文は `MEMO_DIFF_MAX_LINES` 行までにする（残りは件数だけ）
// - 文書（docx・xlsx）の差分は app 層が `core-preview` で行う。ここはテキストだけを扱う

use crate::blob::{read_bytes, Fetched, WORK_REV};
use crate::models::{DiffLineKind, OpsError};
use crate::operations::parse_unified_diff;
use crate::point::resolve_point;
use crate::restore_file::{normalize_project_path, size_at};
use core_git::GitRunner;
use std::borrow::Cow;
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
    let rel = normalize_project_path(path)?;

    // 作業フォルダ側の内容。無い（削除された）ときは None、テキストでないときは本文を渡さない
    let work = match read_bytes(runner, repo, WORK_REV, path, max_bytes) {
        Ok(Fetched::Bytes(bytes)) => match decode_memo_text(&bytes) {
            Some(text) => Some(text),
            None => return Ok(None),
        },
        Ok(Fetched::TooLarge) => return Ok(None),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let head = match resolve_point(runner, repo, "this") {
        Ok(oid) => Some(oid),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    // 最初の保存の前。作業フォルダの内容を全部追加として扱う
    let Some(head) = head else {
        return Ok(work.map(|text| all_added(&text)));
    };
    // 保存（HEAD）に無いファイル（未追跡の新規ファイル）は、作業フォルダの内容を全部追加として扱う
    if size_at(runner, repo, &head, &rel)?.is_none() {
        return Ok(work.map(|text| all_added(&text)));
    }

    // 保存にあるファイル: git の差分をバイト列のまま読み、同じ文字コードの規則で解釈する
    let spec = format!(":(literal){rel}");
    let out = runner.run_ok(
        repo,
        &[
            "diff",
            "--unified=3",
            "--ignore-cr-at-eol",
            &head,
            "--",
            &spec,
        ],
    )?;
    let Some(text) = decode_diff_output(&out.stdout) else {
        return Ok(None);
    };
    // バイナリ（保存時点か作業フォルダのどちらか）は行が出ないため、件数の 0 として扱わず本文を渡さない
    if text.lines().any(|line| line.starts_with("Binary files ")) {
        return Ok(None);
    }
    let lines = parse_unified_diff(&text);
    Ok(Some(collect(
        lines.iter().map(|l| (l.kind, l.content.as_str())),
    )))
}

/// 保存メモの本文として読める文字列にする。
///
/// - NUL を含むもの（バイナリ）は読まない
/// - UTF-8 として有効なら、先頭の BOM（U+FEFF）を除いて使う
/// - それ以外は Shift_JIS として、すべての並びが有効なときだけ使う（Windows の既定の文字コードの日本語ファイル）
/// - どちらでもないものは None
pub(crate) fn decode_memo_text(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.strip_prefix('\u{feff}').unwrap_or(text).to_string());
    }
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(Cow::into_owned)
}

/// `git diff` の出力を、行ごとに読む。保存時点と作業フォルダで文字コードが違う（UTF-8 から Shift_JIS へ変えた）と、
/// 1 つの出力に両方が混ざるため、出力全体では決められない。改行（0x0A）はどちらの文字コードでも 1 バイトなので、行で分けて安全。
fn decode_diff_output(bytes: &[u8]) -> Option<String> {
    let mut text = String::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        text.push_str(&decode_memo_text(line)?);
    }
    Some(text)
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
