// 拡張の配信（`ext://`、設計書 11 章の表示拡張）で使う、パスの解決と Content-Type の許可リスト。純関数。
//
// - 入口は 2 段: `decode_asset_path` で URL のエンコードを外し、`resolve_asset` で拡張フォルダの中の通常のファイルに解決する
// - 解決は字句の検査（`..`・絶対パス・`\`・ドライブ・NUL・空の要素を拒否）のあとに、実体（シンボリックリンクを含む）が
//   拡張フォルダの中にあることを確かめる。外へ出るリンクは `Outside` にする
// - 配信するのは `ASSET_MIME_TYPES` の拡張子だけ（wasm は含めない）
// - 失敗の理由は種類だけを返し、パス・内容は含めない

use std::path::{Path, PathBuf};

/// 配信するファイルの大きさの上限（バイト）
pub const ASSET_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// 相対パスの長さの上限（バイト）。これを超えるものは字句の検査で拒否する
const REL_MAX_LEN: usize = 1024;

/// 配信の許可リスト（拡張子 → Content-Type）。ここに無い拡張子は配信しない
const ASSET_MIME_TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json; charset=utf-8"),
    ("woff2", "font/woff2"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("txt", "text/plain; charset=utf-8"),
];

/// 配信の失敗の理由（応答の状態は呼び出し側が決める）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetError {
    /// 形式が不正（エンコードの誤り、`..`、絶対パス、`\`、ドライブ指定、NUL、空の要素）
    Rejected,
    /// 拡張フォルダの外を指す（シンボリックリンクを含む）
    Outside,
    /// 無い、または通常のファイルではない
    NotFound,
    /// 配信しない種類（許可リストに無い拡張子。wasm を含む）
    NotAllowed,
    /// 配信の上限（16 MiB）を超える
    TooLarge,
}

/// URL のパス部分（パーセントエンコードのまま）を、相対パスに戻す。
/// `%` の後ろが 16 進 2 桁でないもの、デコード後が UTF-8 でないものは拒否する。
/// デコード後の `..` や `/` は、`resolve_asset` が字句の検査で拒否する
pub fn decode_asset_path(raw: &str) -> Result<String, AssetError> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_value);
            let lo = bytes.get(i + 2).copied().and_then(hex_value);
            match (hi, lo) {
                (Some(h), Some(l)) => out.push(h * 16 + l),
                _ => return Err(AssetError::Rejected),
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| AssetError::Rejected)
}

/// 16 進 1 桁の値（大文字・小文字の両方）
fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 相対パス（デコード済み）を、拡張フォルダの中の通常のファイルの実体に解決する
pub fn resolve_asset(ext_root: &Path, rel: &str) -> Result<PathBuf, AssetError> {
    if rel.is_empty() || rel.len() > REL_MAX_LEN {
        return Err(AssetError::Rejected);
    }
    // バックスラッシュ（Windows の区切り）、ドライブ指定（`C:`）、NUL、先頭の `/` を拒否する
    if rel.contains(['\\', ':', '\0']) || rel.starts_with('/') {
        return Err(AssetError::Rejected);
    }
    // 空の要素（`a//b`、末尾の `/`）と `.`・`..` を拒否する
    if rel
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(AssetError::Rejected);
    }
    let root = ext_root.canonicalize().map_err(|_| AssetError::NotFound)?;
    let resolved = root
        .join(rel)
        .canonicalize()
        .map_err(|_| AssetError::NotFound)?;
    // 要素単位で比べる（文字列の前方一致では `ext-x` が `ext` の中と誤認されるため）
    if !resolved.starts_with(&root) {
        return Err(AssetError::Outside);
    }
    if !resolved.is_file() {
        return Err(AssetError::NotFound);
    }
    Ok(resolved)
}

/// 相対パスの拡張子から Content-Type を決める（許可リストに無ければ None）。大文字小文字は区別しない
pub fn asset_mime_of(rel: &str) -> Option<&'static str> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let dot = name.rfind('.')?;
    let ext = name[dot + 1..].to_ascii_lowercase();
    ASSET_MIME_TYPES
        .iter()
        .find(|(known, _)| *known == ext)
        .map(|(_, mime)| *mime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// テスト用の拡張フォルダ（`ui/index.html` と、外のファイルを含む一時フォルダ）
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let root = base.path().join("ext");
        fs::create_dir_all(root.join("ui")).unwrap_or_else(|e| panic!("mkdir: {e}"));
        fs::write(root.join("ui").join("index.html"), b"<!doctype html>")
            .unwrap_or_else(|e| panic!("write: {e}"));
        fs::write(root.join("extract.wasm"), b"\0asm").unwrap_or_else(|e| panic!("write: {e}"));
        fs::write(base.path().join("secret.txt"), b"outside")
            .unwrap_or_else(|e| panic!("write: {e}"));
        // 拡張フォルダの名前と前方一致する兄弟フォルダ（`ext-x`）。要素単位の比較で外と判定できることを確かめる
        fs::create_dir_all(base.path().join("ext-x")).unwrap_or_else(|e| panic!("mkdir: {e}"));
        fs::write(base.path().join("ext-x").join("a.txt"), b"x")
            .unwrap_or_else(|e| panic!("write: {e}"));
        (base, root)
    }

    #[test]
    fn a_file_inside_the_folder_resolves() {
        let (_base, root) = fixture();
        let path = resolve_asset(&root, "ui/index.html").unwrap_or_else(|e| panic!("{e:?}"));
        assert!(path.ends_with("index.html"));
        assert!(path.is_file());
    }

    #[test]
    fn traversal_absolute_drive_backslash_nul_and_empty_segments_are_rejected() {
        let (_base, root) = fixture();
        for rel in [
            "..",
            "../secret.txt",
            "ui/../../secret.txt",
            "ui/./index.html",
            "/secret.txt",
            "C:secret.txt",
            "C:/secret.txt",
            "ui\\index.html",
            "..\\secret.txt",
            "ui/index.html\0",
            "ui//index.html",
            "ui/",
            "",
        ] {
            assert_eq!(
                resolve_asset(&root, rel),
                Err(AssetError::Rejected),
                "{rel:?} should be rejected"
            );
        }
    }

    #[test]
    fn a_too_long_path_is_rejected() {
        let (_base, root) = fixture();
        let long = "a".repeat(REL_MAX_LEN + 1);
        assert_eq!(resolve_asset(&root, &long), Err(AssetError::Rejected));
    }

    #[test]
    fn encoded_traversal_is_rejected_after_decoding() {
        let (_base, root) = fixture();
        // %2e%2e は `..` に戻るため、字句の検査で拒否する
        let decoded = decode_asset_path("%2e%2e/secret.txt").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(decoded, "../secret.txt");
        assert_eq!(resolve_asset(&root, &decoded), Err(AssetError::Rejected));
        // %2F と %5C もデコード後に区切りとして効くため、同様に拒否する
        for raw in [
            "..%2fsecret.txt",
            "..%5csecret.txt",
            "%2fsecret.txt",
            "ui%00",
        ] {
            let decoded = decode_asset_path(raw).unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(
                resolve_asset(&root, &decoded),
                Err(AssetError::Rejected),
                "{raw} should be rejected"
            );
        }
    }

    #[test]
    fn a_bad_percent_escape_or_invalid_utf8_is_rejected_by_decoding() {
        assert_eq!(decode_asset_path("a%2"), Err(AssetError::Rejected));
        assert_eq!(decode_asset_path("a%zz"), Err(AssetError::Rejected));
        assert_eq!(decode_asset_path("%ff"), Err(AssetError::Rejected));
        assert_eq!(
            decode_asset_path("ui/%E6%97%A5.html").unwrap_or_default(),
            "ui/日.html"
        );
    }

    #[test]
    fn a_missing_file_or_a_folder_is_not_found() {
        let (_base, root) = fixture();
        assert_eq!(
            resolve_asset(&root, "ui/none.html"),
            Err(AssetError::NotFound)
        );
        assert_eq!(resolve_asset(&root, "ui"), Err(AssetError::NotFound));
        // 兄弟フォルダへの脱出は `..` の字句の検査で拒否する
        assert_eq!(
            resolve_asset(&root, "../ext-x/a.txt"),
            Err(AssetError::Rejected)
        );
    }

    #[test]
    fn a_symlink_that_leaves_the_folder_is_refused() {
        let (base, root) = fixture();
        let link = root.join("ui").join("out.txt");
        // Windows では権限が無いとシンボリックリンクを作れないため、作れないときは検査をスキップする
        let made = create_file_symlink(&base.path().join("secret.txt"), &link);
        if let Err(err) = made {
            eprintln!("シンボリックリンクを作れないため、この検査はスキップ: {err}");
            return;
        }
        assert_eq!(resolve_asset(&root, "ui/out.txt"), Err(AssetError::Outside));
    }

    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[test]
    fn only_the_allow_list_is_served() {
        assert_eq!(
            asset_mime_of("ui/index.html"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(
            asset_mime_of("ui/assets/app.MJS"),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(asset_mime_of("font.woff2"), Some("font/woff2"));
        assert_eq!(asset_mime_of("a.png"), Some("image/png"));
        for rel in [
            "extract.wasm",
            "extension.exe",
            "noext",
            "a.docx",
            "a.js.map",
            "x.PDF",
        ] {
            assert_eq!(asset_mime_of(rel), None, "{rel} should not be served");
        }
    }
}
