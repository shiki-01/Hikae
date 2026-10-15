// パスと名前の検査（設計書 14.5）。純関数だけを置き、通信より前に呼ぶ。
// 名前は Unicode を NFC に正規化してから検査し、正規化後の値を返す（macOS と Windows の差を避ける）。

use crate::error::RemoteError;
use core_github::{validate_owner_login, validate_repository_name};
use unicode_normalization::UnicodeNormalization;

/// 1 階層の名前の長さの上限（文字数）
const MAX_NAME_CHARS: usize = 255;
/// パス全体の長さの上限（文字数。Windows の従来の上限に合わせる）
const MAX_PATH_CHARS: usize = 259;

/// Windows で予約されている名前（拡張子を付けても、大文字小文字を変えても予約）
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
    "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9",
];

fn invalid(reason: &str) -> RemoteError {
    RemoteError::InvalidPath(reason.to_string())
}

/// 保存先の所有者とリポジトリ名を検査する。GitHub の規則に合わない文字は通信前に拒否する。
pub(crate) fn check_repo(owner: &str, name: &str) -> Result<(), RemoteError> {
    validate_owner_login(owner).map_err(|_| invalid("保存先の名前が GitHub の規則に合いません"))?;
    validate_repository_name(name)
        .map_err(|_| invalid("リポジトリ名が GitHub の規則に合いません"))?;
    Ok(())
}

/// 1 階層のファイル名・フォルダ名を検査し、NFC に正規化した値を返す。
pub fn check_name(name: &str) -> Result<String, RemoteError> {
    let n: String = name.nfc().collect();
    if n.is_empty() {
        return Err(invalid("名前が空です"));
    }
    if n.chars().count() > MAX_NAME_CHARS {
        return Err(invalid("名前が長すぎます（255 文字まで）"));
    }
    if n == "." || n == ".." {
        return Err(invalid("使えない名前です"));
    }
    if n.eq_ignore_ascii_case(".git") {
        return Err(invalid(".git という名前は使えません"));
    }
    if n.chars().any(char::is_control) {
        return Err(invalid("制御文字を含んでいます"));
    }
    if n.chars()
        .any(|c| matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(invalid("使えない文字を含んでいます"));
    }
    if n.ends_with('.') || n.ends_with(char::is_whitespace) {
        return Err(invalid("末尾に . や空白は使えません"));
    }
    // 拡張子の前の部分で予約名を判定する（`CON.txt` も予約）
    let stem = n.split('.').next().unwrap_or_default();
    let stem_upper = stem.trim_end().to_ascii_uppercase();
    if RESERVED_NAMES.contains(&stem_upper.as_str()) {
        return Err(invalid("Windows の予約名です"));
    }
    Ok(n)
}

/// 保存先のフォルダを検査し、先頭・末尾の `/` を取り除いた値を返す。空はルート。
pub fn check_dir(dir: &str) -> Result<String, RemoteError> {
    let normalized: String = dir.nfc().collect();
    let trimmed = normalized.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    for segment in trimmed.split('/') {
        check_name(segment)?;
    }
    if trimmed.chars().count() > MAX_PATH_CHARS {
        return Err(invalid("パスが長すぎます（259 文字まで）"));
    }
    Ok(trimmed.to_string())
}

/// 保存先のフォルダと名前から、リポジトリのルートからのパスを作る。全体の長さを検査する。
pub fn check_target_path(dir: &str, name: &str) -> Result<String, RemoteError> {
    let path = if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    };
    if path.chars().count() > MAX_PATH_CHARS {
        return Err(invalid("パスが長すぎます（259 文字まで）"));
    }
    Ok(path)
}

/// 閲覧用のパスを検査する（ファイル名の規則は適用しない。読むだけなので `..` と区切りの誤りだけ拒否）。
/// 先頭・末尾の `/` は取り除く。空はルート。
pub fn check_list_path(path: &str) -> Result<String, RemoteError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(invalid("区切りや相対パスの指定が正しくありません"));
        }
        if segment
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | '?' | '#'))
        {
            return Err(invalid("使えない文字を含んでいます"));
        }
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_invalid(r: Result<String, RemoteError>) -> bool {
        matches!(r, Err(RemoteError::InvalidPath(_)))
    }

    #[test]
    fn accepts_ordinary_names() {
        assert_eq!(check_name("photo.jpg").expect("ok"), "photo.jpg");
        assert_eq!(check_name("報告書 (2).pdf").expect("ok"), "報告書 (2).pdf");
        assert_eq!(check_name(".bashrc").expect("ok"), ".bashrc");
        assert_eq!(check_name("CONTACT.txt").expect("ok"), "CONTACT.txt");
        assert_eq!(check_name("COM10").expect("ok"), "COM10");
    }

    #[test]
    fn rejects_empty_dot_and_git_names() {
        assert!(is_invalid(check_name("")));
        assert!(is_invalid(check_name(".")));
        assert!(is_invalid(check_name("..")));
        assert!(is_invalid(check_name(".git")));
        assert!(is_invalid(check_name(".GIT")));
    }

    #[test]
    fn rejects_separators_control_and_forbidden_characters() {
        for bad in [
            "a/b", "a\\b", "a\nb", "a\u{0}b", "a<b", "a>b", "a:b", "a\"b", "a|b", "a?b", "a*b",
        ] {
            assert!(is_invalid(check_name(bad)), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn rejects_trailing_dot_and_whitespace() {
        assert!(is_invalid(check_name("name.")));
        assert!(is_invalid(check_name("name ")));
        assert!(is_invalid(check_name("name\t")));
    }

    #[test]
    fn rejects_windows_reserved_names_in_any_case_and_with_extension() {
        for bad in [
            "CON",
            "con",
            "PRN.txt",
            "aux.tar.gz",
            "NUL",
            "com1",
            "COM9.log",
            "LPT1",
            "lpt9.x",
            "CONIN$",
            "conout$.txt",
        ] {
            assert!(is_invalid(check_name(bad)), "{bad:?} must be rejected");
        }
        // 予約名を含むだけの名前は通す
        assert!(check_name("CONSOLE").is_ok());
        assert!(check_name("my-CON").is_ok());
    }

    #[test]
    fn rejects_names_over_255_chars() {
        let ok = "a".repeat(255);
        assert!(check_name(&ok).is_ok());
        let too_long = "a".repeat(256);
        assert!(is_invalid(check_name(&too_long)));
        // 文字数で数える（マルチバイトも 1 文字）
        let ok_ja = "あ".repeat(255);
        assert!(check_name(&ok_ja).is_ok());
    }

    #[test]
    fn normalizes_names_to_nfc() {
        // NFD の「が」（か + 濁点）は NFC の 1 文字になる
        let nfd = "か\u{3099}.jpg";
        assert_eq!(check_name(nfd).expect("ok"), "が.jpg");
    }

    #[test]
    fn dirs_strip_outer_slashes_and_check_each_segment() {
        assert_eq!(check_dir("").expect("root"), "");
        assert_eq!(check_dir("/").expect("root"), "");
        assert_eq!(check_dir("/photos/2026/").expect("ok"), "photos/2026");
        assert_eq!(check_dir("a/b").expect("ok"), "a/b");
        assert!(is_invalid(check_dir("a//b")));
        assert!(is_invalid(check_dir("a/../b")));
        assert!(is_invalid(check_dir("a/.git")));
        assert!(is_invalid(check_dir("a/CON")));
        assert!(is_invalid(check_dir("a\\b")));
    }

    #[test]
    fn dirs_normalize_to_nfc() {
        let nfd = "か\u{3099}";
        assert_eq!(check_dir(nfd).expect("ok"), "が");
    }

    #[test]
    fn full_target_path_is_limited_to_259_chars() {
        let dir = "d".repeat(250);
        // dir(250) + "/" + name(8) = 259 は通る
        assert!(check_target_path(&dir, "abcdefgh").is_ok());
        // 260 は拒否
        assert!(is_invalid(check_target_path(&dir, "abcdefghi")));
        // 空のフォルダ（ルート）は名前だけで判定する
        assert_eq!(check_target_path("", "a.txt").expect("ok"), "a.txt");
    }

    #[test]
    fn dir_over_259_chars_is_rejected() {
        let long = (0..30).map(|_| "abcdefghi").collect::<Vec<_>>().join("/");
        assert!(long.chars().count() > 259);
        assert!(is_invalid(check_dir(&long)));
    }

    #[test]
    fn list_paths_allow_normal_paths_and_reject_traversal() {
        assert_eq!(check_list_path("").expect("root"), "");
        assert_eq!(check_list_path("/docs/a b.md/").expect("ok"), "docs/a b.md");
        assert!(is_invalid(check_list_path("docs/../x")));
        assert!(is_invalid(check_list_path("./x")));
        assert!(is_invalid(check_list_path("a//b")));
        assert!(is_invalid(check_list_path("a?b=1")));
        assert!(is_invalid(check_list_path("a#b")));
        assert!(is_invalid(check_list_path("a\\b")));
    }

    #[test]
    fn repo_names_follow_github_rules() {
        assert!(check_repo("alice", "notes.v2").is_ok());
        assert!(check_repo("alice/evil", "notes").is_err());
        assert!(check_repo("alice", "notes?x=1").is_err());
        assert!(check_repo("alice", "../x").is_err());
        assert!(check_repo("alice", "notes.git").is_err());
    }
}
