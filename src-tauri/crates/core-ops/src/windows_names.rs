// 取り込み前のファイル名の検査（設計書 5章 E18）。
//
// 別の PC（macOS など）で作られたファイルのうち、Windows では作れない名前があると、取り込みの途中で
// 失敗して作業フォルダが中途半端になりうる。そこで、取り込む側のツリーのパスを `fetch` の後・`merge` の前に
// 調べ、作れない名前があれば、何も取り込まずに知らせる。検査は Windows でだけ行う。
//
// 判定は文字列だけを見る純関数（`find_unsupported_names`）で、OS には依存しない。
// 見つけるのは次の 4 種類。回避は「元の PC で名前を変えて保存し直す」案内とする
// （git 側で該当ファイルだけを飛ばすことはできない。`core.protectNTFS` などの既定値は維持する）。
//
// - 予約名: `CON`、`PRN`、`AUX`、`NUL`、`COM1`〜`COM9`、`LPT1`〜`LPT9`（拡張子が付いても同じ。`Aux.txt`）
// - 末尾が `.` または空白の名前
// - 使えない文字（`< > : " | ? * \` と制御文字）
// - 長すぎるパス（プロジェクトのフォルダを含めて 259 文字を超える、または 1 つの名前が 255 文字を超える）

use crate::models::OpsError;
use core_git::GitRunner;
use std::path::Path;

/// Windows で扱えるパスの最大の長さ（`MAX_PATH` 260 から終端の NUL を除く）
pub const WINDOWS_MAX_PATH_CHARS: usize = 259;
/// Windows で扱えるファイル名・フォルダ名 1 つの最大の長さ
pub const WINDOWS_MAX_COMPONENT_CHARS: usize = 255;
/// 利用者に示す一覧の最大の件数（それ以上は件数だけを伝える）
pub const MAX_LISTED_NAMES: usize = 50;

/// 作れない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    /// Windows の予約名
    ReservedName,
    /// 末尾が `.` または空白
    TrailingDotOrSpace,
    /// 使えない文字を含む
    InvalidCharacter,
    /// パスまたは名前が長すぎる
    TooLong,
}

/// Windows では作れないパス
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedName {
    /// プロジェクトからの相対パス（`/` 区切り）
    pub path: String,
    pub reason: UnsupportedReason,
}

/// 予約名か。拡張子（最初の `.` 以降）と、名前の末尾の空白は無視する。大文字小文字は区別しない。
fn is_reserved_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ');
    let upper = stem.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    // COM1〜COM9、LPT1〜LPT9（上付きの ¹²³ も Windows は同じ扱いにする）
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let mut chars = rest.chars();
            if let (Some(digit), None) = (chars.next(), chars.next()) {
                if matches!(digit, '1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}') {
                    return true;
                }
            }
        }
    }
    false
}

/// 使えない文字か（`\` は git のパスでは区切りではなく、Windows では区切りになってしまう）
fn is_invalid_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\') || (c as u32) < 0x20
}

/// UTF-16 での長さ（Windows のパスの長さの数え方）
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// 1 つのパスの問題。問題が無ければ `None`。最初に見つかった 1 つだけを返す。
fn problem_of(path: &str, root_len: usize) -> Option<UnsupportedReason> {
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.iter().any(|c| c.chars().any(is_invalid_char)) {
        return Some(UnsupportedReason::InvalidCharacter);
    }
    if components.iter().any(|c| is_reserved_name(c)) {
        return Some(UnsupportedReason::ReservedName);
    }
    if components
        .iter()
        .any(|c| c.ends_with('.') || c.ends_with(' '))
    {
        return Some(UnsupportedReason::TrailingDotOrSpace);
    }
    // プロジェクトのフォルダ + 区切り + 相対パス
    let total = root_len + 1 + utf16_len(path);
    if total > WINDOWS_MAX_PATH_CHARS
        || components
            .iter()
            .any(|c| utf16_len(c) > WINDOWS_MAX_COMPONENT_CHARS)
    {
        return Some(UnsupportedReason::TooLong);
    }
    None
}

/// プロジェクトのフォルダのパスの長さ（UTF-16。末尾の区切りは数えない）
pub fn root_path_len(root: &Path) -> usize {
    utf16_len(root.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// Windows では作れない名前を探す（純関数）。`root_len` はプロジェクトのフォルダのパスの長さ。
/// 入力の順序を保ち、問題のあるパスだけを返す。
pub fn find_unsupported_names<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    root_len: usize,
) -> Vec<UnsupportedName> {
    paths
        .into_iter()
        .filter_map(|path| {
            problem_of(path, root_len).map(|reason| UnsupportedName {
                path: path.to_string(),
                reason,
            })
        })
        .collect()
}

/// 取り込む側（`@{u}`）のツリーを検査し、作れない名前があれば `UnsupportedFileNames` で断る。
/// 読み取りだけで、作業フォルダ・インデックスは変えない。
pub(crate) fn check_incoming_tree(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    let out = runner.run_ok(repo, &["ls-tree", "-r", "-z", "--name-only", "@{u}"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let found = find_unsupported_names(
        text.split('\0').filter(|p| !p.is_empty()),
        root_path_len(repo),
    );
    if found.is_empty() {
        Ok(())
    } else {
        Err(OpsError::UnsupportedFileNames(found))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(path: &str) -> Option<UnsupportedReason> {
        problem_of(path, 10)
    }

    #[test]
    fn reserved_names_are_found_with_or_without_extensions() {
        for path in [
            "Aux.txt",
            "aux",
            "AUX.tar.gz",
            "docs/NUL.md",
            "docs/con/readme.txt",
            "Com1.txt",
            "lpt9",
            "COM\u{b9}.txt",
            "CONIN$",
            "aux .txt",
        ] {
            assert_eq!(
                reason(path),
                Some(UnsupportedReason::ReservedName),
                "{path}"
            );
        }
    }

    #[test]
    fn near_misses_of_reserved_names_are_allowed() {
        for path in [
            "auxiliary.txt",
            "com0.txt",
            "com10.txt",
            "COMM1.txt",
            "lpt.txt",
            "console.log",
            "nul-terminated.txt",
            "my.aux.txt",
            "第3章.docx",
        ] {
            assert_eq!(reason(path), None, "{path}");
        }
    }

    #[test]
    fn trailing_dots_and_spaces_are_found_in_any_component() {
        for path in ["memo.", "memo ", "docs./a.txt", "docs /a.txt", "a/b/c ."] {
            assert_eq!(
                reason(path),
                Some(UnsupportedReason::TrailingDotOrSpace),
                "{path}"
            );
        }
        // 先頭のドットや途中の空白は問題ない
        for path in [".gitignore", "a b.txt", "docs/.hidden", "報告 書.docx"] {
            assert_eq!(reason(path), None, "{path}");
        }
    }

    #[test]
    fn invalid_characters_are_found() {
        for path in [
            "a<b.txt",
            "a>b.txt",
            "a:b.txt",
            "a\"b.txt",
            "a|b.txt",
            "a?b.txt",
            "a*b.txt",
            "a\\b.txt",
            "a\u{1}b.txt",
            "docs/メモ:1.txt",
        ] {
            assert_eq!(
                reason(path),
                Some(UnsupportedReason::InvalidCharacter),
                "{path}"
            );
        }
    }

    #[test]
    fn the_first_problem_decides_the_reason() {
        // 使えない文字と予約名の両方があるときは、使えない文字を先に報告する
        assert_eq!(
            reason("aux/a:b.txt"),
            Some(UnsupportedReason::InvalidCharacter)
        );
    }

    #[test]
    fn long_paths_depend_on_the_project_folder_length() {
        let name = "a".repeat(200);
        // 10 + 1 + 200 = 211 文字。余裕がある
        assert_eq!(problem_of(&name, 10), None);
        // 259 文字ちょうどは可、260 文字は不可
        let exact = "b".repeat(WINDOWS_MAX_PATH_CHARS - 10 - 1);
        assert_eq!(problem_of(&exact, 10), None);
        let over = "b".repeat(WINDOWS_MAX_PATH_CHARS - 10);
        assert_eq!(problem_of(&over, 10), Some(UnsupportedReason::TooLong));
        // 同じ相対パスでも、フォルダが深いと不可になる
        assert_eq!(problem_of(&name, 100), Some(UnsupportedReason::TooLong));
    }

    #[test]
    fn a_single_name_over_255_utf16_units_is_too_long() {
        let name = "c".repeat(256);
        assert_eq!(problem_of(&name, 0), Some(UnsupportedReason::TooLong));
        let ok = "c".repeat(255);
        assert_eq!(problem_of(&ok, 0), None);
        // 日本語は 1 文字 = UTF-16 の 1 単位。絵文字などは 2 単位
        let wide = "😀".repeat(128);
        assert_eq!(problem_of(&wide, 0), Some(UnsupportedReason::TooLong));
    }

    #[test]
    fn finds_only_the_bad_paths_in_order() {
        let paths = [
            "README.md",
            "Aux.txt",
            "docs/ok.docx",
            "memo.",
            "src/main.rs",
        ];
        let found = find_unsupported_names(paths, 10);
        assert_eq!(
            found,
            vec![
                UnsupportedName {
                    path: "Aux.txt".to_string(),
                    reason: UnsupportedReason::ReservedName
                },
                UnsupportedName {
                    path: "memo.".to_string(),
                    reason: UnsupportedReason::TrailingDotOrSpace
                },
            ]
        );
        assert!(find_unsupported_names(["a.txt", "b/c.txt"], 10).is_empty());
        assert!(find_unsupported_names(std::iter::empty::<&str>(), 10).is_empty());
    }

    #[test]
    fn root_length_ignores_trailing_separators() {
        assert_eq!(root_path_len(Path::new("C:\\work\\proj")), 12);
        assert_eq!(root_path_len(Path::new("C:\\work\\proj\\")), 12);
        assert_eq!(root_path_len(Path::new("/home/a/proj/")), 12);
        assert_eq!(root_path_len(Path::new("C:\\資料\\卒論")), 8);
    }
}
