// カスタム GGUF（利用者が自分で用意したモデルファイル）の検証と ID（設計 7.1・10.5）。
//
// - 読み取りのみ。ファイルはコピー・移動・書き換えしない（パスの参照だけ）
// - SHA-256 は検証しない。モデルの内容とライセンスは利用者の責任（画面に注意書きを出す）
// - 理由は列挙値で返す。パス・ファイルの内容は、エラーにもログにも出さない
// - モデル ID（キャッシュのキー）はパスから作るが、パスそのものは含めない

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// 読み込みを許すファイルの上限（バイト）。12GB を超えるものは拒否する
pub const MAX_CUSTOM_GGUF_BYTES: u64 = 12 * 1024 * 1024 * 1024;

/// GGUF ファイルの先頭 4 バイト（マジックナンバー）
const GGUF_MAGIC: &[u8; 4] = b"GGUF";

/// カスタム GGUF が使えない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomGgufError {
    /// パスが空
    Empty,
    /// パスに制御文字が含まれる
    ControlChar,
    /// 存在しない、または参照できない
    NotFound,
    /// シンボリックリンク（拒否する。実体のパスを確かめられないため）
    Link,
    /// 通常のファイルではない（フォルダなど）
    NotAFile,
    /// 拡張子が `.gguf` ではない
    WrongExtension,
    /// 上限（12GB）を超える
    TooLarge,
    /// 先頭が `GGUF` ではない（ファイルが短すぎる場合を含む）
    NotGguf,
    /// 先頭を読めない（権限など）
    Unreadable,
}

/// 検証を通ったカスタム GGUF
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomGguf {
    /// ファイルのサイズ（バイト）
    pub size_bytes: u64,
}

/// カスタム GGUF を検証する（読み取りのみ）。
/// 順序: パスの形、存在と種類（リンクを含む）、拡張子、サイズ、先頭のマジックナンバー。
pub fn validate_custom_gguf(path: &Path) -> Result<CustomGguf, CustomGgufError> {
    let text = path.to_string_lossy();
    if text.is_empty() {
        return Err(CustomGgufError::Empty);
    }
    if text.chars().any(char::is_control) {
        return Err(CustomGgufError::ControlChar);
    }
    // リンクを辿らずに調べる（リンク先の実体は見ない）
    let meta = std::fs::symlink_metadata(path).map_err(|_| CustomGgufError::NotFound)?;
    if meta.file_type().is_symlink() {
        return Err(CustomGgufError::Link);
    }
    if !meta.is_file() {
        return Err(CustomGgufError::NotAFile);
    }
    let has_gguf_extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"));
    if !has_gguf_extension {
        return Err(CustomGgufError::WrongExtension);
    }
    let size_bytes = meta.len();
    check_size(size_bytes)?;
    let mut file = std::fs::File::open(path).map_err(|_| CustomGgufError::Unreadable)?;
    let mut magic = [0u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(CustomGgufError::NotGguf)
        }
        Err(_) => return Err(CustomGgufError::Unreadable),
    }
    if &magic != GGUF_MAGIC {
        return Err(CustomGgufError::NotGguf);
    }
    Ok(CustomGguf { size_bytes })
}

/// サイズの上限を確かめる（実ファイルを作らずに判定できるように分けている）
fn check_size(size_bytes: u64) -> Result<(), CustomGgufError> {
    if size_bytes > MAX_CUSTOM_GGUF_BYTES {
        return Err(CustomGgufError::TooLarge);
    }
    Ok(())
}

/// カスタム GGUF のモデル ID（キャッシュのキーに使う）。`custom:` と、SHA-256 の先頭 12 桁。
/// ハッシュの入力は、パスと、ファイルのサイズ・更新時刻（読めれば）。同じパスのファイルを別のモデルに
/// 差し替えたとき、古いモデルの提案がキャッシュから返らないようにするため。
/// パスそのものは、キーにもログにも出さない。
pub fn custom_model_id(path: &Path) -> String {
    let mut input = path.to_string_lossy().into_owned();
    if let Ok(meta) = std::fs::metadata(path) {
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        input.push_str(&format!("|{}|{modified}", meta.len()));
    }
    let digest = Sha256::digest(input.as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("custom:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 先頭に指定のバイト列を書いた一時ファイルを作る
    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn gguf_bytes() -> Vec<u8> {
        let mut v = b"GGUF".to_vec();
        v.extend_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    #[test]
    fn accepts_a_regular_gguf_file_and_reports_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "model.gguf", &gguf_bytes());
        let ok = validate_custom_gguf(&path).unwrap();
        assert_eq!(ok.size_bytes, 12);
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "MODEL.GGUF", &gguf_bytes());
        assert!(validate_custom_gguf(&path).is_ok());
    }

    #[test]
    fn rejects_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("none.gguf");
        assert_eq!(validate_custom_gguf(&path), Err(CustomGgufError::NotFound));
    }

    #[test]
    fn rejects_empty_path() {
        assert_eq!(
            validate_custom_gguf(Path::new("")),
            Err(CustomGgufError::Empty)
        );
    }

    #[test]
    fn rejects_control_characters_in_path() {
        assert_eq!(
            validate_custom_gguf(Path::new("a\u{7}b.gguf")),
            Err(CustomGgufError::ControlChar)
        );
        assert_eq!(
            validate_custom_gguf(Path::new("a\nb.gguf")),
            Err(CustomGgufError::ControlChar)
        );
    }

    #[test]
    fn rejects_directory_named_like_a_model() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder.gguf");
        fs::create_dir(&folder).unwrap();
        assert_eq!(
            validate_custom_gguf(&folder),
            Err(CustomGgufError::NotAFile)
        );
    }

    #[test]
    fn rejects_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "model.bin", &gguf_bytes());
        assert_eq!(
            validate_custom_gguf(&path),
            Err(CustomGgufError::WrongExtension)
        );
        let no_ext = write_file(dir.path(), "model", &gguf_bytes());
        assert_eq!(
            validate_custom_gguf(&no_ext),
            Err(CustomGgufError::WrongExtension)
        );
    }

    #[test]
    fn rejects_file_without_gguf_magic() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "fake.gguf", b"NOPE1234567890");
        assert_eq!(validate_custom_gguf(&path), Err(CustomGgufError::NotGguf));
    }

    #[test]
    fn rejects_file_shorter_than_magic() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "short.gguf", b"GGU");
        assert_eq!(validate_custom_gguf(&path), Err(CustomGgufError::NotGguf));
        let empty = write_file(dir.path(), "empty.gguf", b"");
        assert_eq!(validate_custom_gguf(&empty), Err(CustomGgufError::NotGguf));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_link_to_a_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = write_file(dir.path(), "real.gguf", &gguf_bytes());
        let link = dir.path().join("link.gguf");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(validate_custom_gguf(&link), Err(CustomGgufError::Link));
    }

    #[test]
    fn size_limit_is_12_gib_and_inclusive() {
        // 実ファイルは作らず、判定の関数で確かめる（12GB ちょうどは通し、超えたものは拒否する）
        assert_eq!(MAX_CUSTOM_GGUF_BYTES, 12 * 1024 * 1024 * 1024);
        assert_eq!(check_size(MAX_CUSTOM_GGUF_BYTES), Ok(()));
        assert_eq!(
            check_size(MAX_CUSTOM_GGUF_BYTES + 1),
            Err(CustomGgufError::TooLarge)
        );
    }

    #[test]
    fn model_id_is_prefixed_and_hides_the_path() {
        let id = custom_model_id(Path::new("C:\\models\\secret-name.gguf"));
        assert!(id.starts_with("custom:"));
        assert_eq!(id.len(), "custom:".len() + 12);
        assert!(id[7..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!id.contains("secret"));
        // 同じパスは同じ ID、違うパスは違う ID
        assert_eq!(
            id,
            custom_model_id(Path::new("C:\\models\\secret-name.gguf"))
        );
        assert_ne!(id, custom_model_id(Path::new("C:\\models\\other.gguf")));
    }
}
