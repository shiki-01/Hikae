// 画像などの内容を読み取る（設計書 9.3 `app-blob`、3.3 S4 の画像比較）。読み取りのみ。
//
// - `rev` は完全な OID（40 または 64 桁の 16 進）か、作業フォルダの現在のファイルを表す `work` だけを受け付ける
// - パスは `normalize_project_path`（`export_file_at` と同じ字句検査）に加えて、制御文字を拒否する
// - 許可するのは画像の拡張子と pdf（svg は含めない）。mime は拡張子から決め、中身は推測しない
// - `read_text` は拡張子を見ず、UTF-8 として有効で NUL を含まない小さいファイルだけを文字列として返す（プレビュー用）
// - 作業フォルダ・インデックス・refs には一切書かない（復元点は不要。不変条件 3 の対象外）
// - git は `ls-tree`・`cat-file` だけを使う（許可リストは変更しない）

use crate::models::OpsError;
use crate::open_path::{resolve_in_project, OpenPathError};
use crate::restore_file::{normalize_project_path, resolve_commit};
use core_git::GitRunner;
use std::io::Read;
use std::path::Path;

/// 読み取った内容と mime
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// 作業フォルダの現在のファイルを表す `rev`
pub const WORK_REV: &str = "work";

/// 拡張子（小文字）から mime を決める。対応しない拡張子は None
fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

/// 完全な OID（40 または 64 桁の 16 進）か
pub(crate) fn is_full_oid(rev: &str) -> bool {
    (rev.len() == 40 || rev.len() == 64) && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 読み取りの結果。上限を超えたものは、エラーではなく `TooLarge` として返す
/// （画像は `InvalidInput` に、Office 文書は抽出できない理由に写す）
enum Fetched {
    Bytes(Vec<u8>),
    TooLarge,
}

/// Office 文書（設計書 3.3 S4）を読んだ結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeFile {
    /// 読んだ内容（バイト列そのまま）
    Bytes(Vec<u8>),
    /// 上限（`max_bytes`）を超えたため、読まなかった
    TooLarge,
}

/// 作業フォルダの `work`、または完全な OID だけを受け付けるかを確かめ、パスを正規化する。
/// 制御文字を含むパス、プロジェクトの外へ出るパス、`.git` 配下のパスは拒否する
fn validate_request(rev: &str, path: &str) -> Result<String, OpsError> {
    if rev != WORK_REV && !is_full_oid(rev) {
        return Err(OpsError::InvalidInput(
            "rev must be a full object id or work".to_string(),
        ));
    }
    if path.chars().any(char::is_control) {
        return Err(OpsError::InvalidInput(
            "path must not contain control characters".to_string(),
        ));
    }
    normalize_project_path(path)
}

/// パスの拡張子（小文字）。拡張子が無ければ空
fn extension_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// テキストとして読んだ結果（プレビュー用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFile {
    /// UTF-8 として有効で NUL を含まない内容
    Text(String),
    /// 上の条件に合わない（バイナリ、UTF-8 として無効）
    Binary,
    /// 上限（`max_bytes`）を超えたため、読まなかった
    TooLarge,
}

/// 指定時点（完全な OID）または作業フォルダ（`work`）の画像（pdf を含む）ファイルを読む。
///
/// - 存在しない（その時点に通常のファイルとして無い、作業フォルダに無い）場合は `Io(NotFound)`
/// - 拡張子が画像・pdf でない、パスが不正、リンクで外へ出る、通常のファイルでない場合は `InvalidInput`
/// - `max_bytes` を超えるものは読まずに `InvalidInput` にする
pub(crate) fn read_blob(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    path: &str,
    max_bytes: u64,
) -> Result<Blob, OpsError> {
    let rel = validate_request(rev, path)?;
    let ext = extension_of(&rel);
    let mime = mime_for_extension(&ext)
        .ok_or_else(|| OpsError::InvalidInput("file type is not an allowed blob".to_string()))?;

    match fetch(runner, repo, rev, &rel, max_bytes)? {
        Fetched::Bytes(bytes) => Ok(Blob { bytes, mime }),
        Fetched::TooLarge => Err(OpsError::InvalidInput("file is too large".to_string())),
    }
}

/// 指定時点（完全な OID）または作業フォルダ（`work`）の Office 文書（docx・xlsx・pptx）を読む（設計書 3.3 S4）。
///
/// - 存在しない場合は `Io(NotFound)`。パスの検査・リンクの扱いは `read_blob` と同じ
/// - 拡張子が docx・xlsx・pptx でない場合は `InvalidInput`（`.doc`・`.docm`・`.xls`・`.ppt` を含む）
/// - `max_bytes` を超えるものは読まずに `OfficeFile::TooLarge` を返す
pub(crate) fn read_office(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    path: &str,
    max_bytes: u64,
) -> Result<OfficeFile, OpsError> {
    let rel = validate_request(rev, path)?;
    let ext = extension_of(&rel);
    if ext != "docx" && ext != "xlsx" && ext != "pptx" {
        return Err(OpsError::InvalidInput(
            "file type is not an allowed office document".to_string(),
        ));
    }
    match fetch(runner, repo, rev, &rel, max_bytes)? {
        Fetched::Bytes(bytes) => Ok(OfficeFile::Bytes(bytes)),
        Fetched::TooLarge => Ok(OfficeFile::TooLarge),
    }
}

/// 指定時点（完全な OID）または作業フォルダ（`work`）のファイルを、テキストとして読む（プレビュー用）。
///
/// - 拡張子は見ない。存在しない場合は `Io(NotFound)`。パスの検査・リンクの扱いは `read_blob` と同じ
/// - `max_bytes` を超えるものは読まずに `TextFile::TooLarge`
/// - UTF-8 として有効で NUL を含まないものだけ `TextFile::Text`。それ以外は `TextFile::Binary`
pub(crate) fn read_text(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    path: &str,
    max_bytes: u64,
) -> Result<TextFile, OpsError> {
    let rel = validate_request(rev, path)?;
    match fetch(runner, repo, rev, &rel, max_bytes)? {
        Fetched::TooLarge => Ok(TextFile::TooLarge),
        Fetched::Bytes(bytes) => Ok(classify_text(bytes)),
    }
}

/// 内容がテキストか。NUL を含まず、UTF-8 として有効なら文字列にする
fn classify_text(bytes: Vec<u8>) -> TextFile {
    if bytes.contains(&0) {
        return TextFile::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(text) => TextFile::Text(text),
        Err(_) => TextFile::Binary,
    }
}

/// 時点または作業フォルダの通常のファイルを、上限つきで読む（拡張子の検査は呼び出し側）
fn fetch(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    rel: &str,
    max_bytes: u64,
) -> Result<Fetched, OpsError> {
    if rev == WORK_REV {
        read_work_file(repo, rel, max_bytes)
    } else {
        read_at_commit(runner, repo, rev, rel, max_bytes)
    }
}

/// 指定時点の通常のファイルを読む（`ls-tree` で確認してから `cat-file -p` で読む。`export_file_at` と同じ手順）
fn read_at_commit(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    rel: &str,
    max_bytes: u64,
) -> Result<Fetched, OpsError> {
    let oid = resolve_commit(runner, repo, rev)?;
    let entry = crate::operations::parse_ls_tree_long(
        &runner
            .run_ok(repo, &["ls-tree", "-z", "-l", &oid, "--", rel])?
            .stdout,
    )
    .into_iter()
    .find(|f| f.path == rel)
    .ok_or_else(|| OpsError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))?;

    // シンボリックリンク（mode 120000）も blob として並ぶため、通常のファイルだけを通す
    if entry.mode != "100644" && entry.mode != "100755" {
        return Err(OpsError::InvalidInput(
            "path is not a regular file".to_string(),
        ));
    }
    // 読み取る前に大きさで止める
    if entry.size > max_bytes {
        return Ok(Fetched::TooLarge);
    }
    // ls-tree の大きさと実際の内容が違っても上限を超えないよう、読み取り後にも確認する
    let spec = format!("{oid}:{rel}");
    let bytes = runner.run_ok(repo, &["cat-file", "-p", &spec])?.stdout;
    if bytes.len() as u64 > max_bytes {
        return Ok(Fetched::TooLarge);
    }
    Ok(Fetched::Bytes(bytes))
}

/// 作業フォルダの現在のファイルを読む。リンクで外へ出るもの、通常のファイルでないものは拒否する。
fn read_work_file(repo: &Path, rel: &str, max_bytes: u64) -> Result<Fetched, OpsError> {
    let abs = resolve_in_project(repo, rel).map_err(open_path_to_ops)?;
    let meta = std::fs::metadata(&abs)?;
    if !meta.is_file() {
        return Err(OpsError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )));
    }
    if meta.len() > max_bytes {
        return Ok(Fetched::TooLarge);
    }
    // 読んでいる間に大きくなっても上限を超えないよう、上限 + 1 バイトまでで止める
    let mut bytes = Vec::new();
    std::fs::File::open(&abs)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Ok(Fetched::TooLarge);
    }
    Ok(Fetched::Bytes(bytes))
}

/// `resolve_in_project` の失敗を `OpsError` へ写す。見つからない・通常でないものは NotFound、
/// プロジェクトの外へ出るものは InvalidInput
fn open_path_to_ops(err: OpenPathError) -> OpsError {
    match err {
        OpenPathError::NotFound => OpsError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)),
        OpenPathError::Invalid | OpenPathError::NotRelative | OpenPathError::Outside => {
            OpsError::InvalidInput("path is not allowed".to_string())
        }
        OpenPathError::RootUnavailable(e) | OpenPathError::Io(e) => OpsError::Io(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_oid_is_40_or_64_hex_digits_only() {
        assert!(is_full_oid(&"a".repeat(40)));
        assert!(is_full_oid(&"0F".repeat(20)));
        assert!(is_full_oid(&"b".repeat(64)));
        assert!(!is_full_oid(&"a".repeat(7)));
        assert!(!is_full_oid(&"a".repeat(41)));
        assert!(!is_full_oid(&format!("{}g", "a".repeat(39))));
        assert!(!is_full_oid("HEAD"));
        assert!(!is_full_oid(""));
    }

    #[test]
    fn mime_follows_the_extension_and_svg_is_not_allowed() {
        assert_eq!(mime_for_extension("png"), Some("image/png"));
        assert_eq!(mime_for_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("gif"), Some("image/gif"));
        assert_eq!(mime_for_extension("webp"), Some("image/webp"));
        assert_eq!(mime_for_extension("bmp"), Some("image/bmp"));
        assert_eq!(mime_for_extension("pdf"), Some("application/pdf"));
        assert_eq!(mime_for_extension("svg"), None);
        assert_eq!(mime_for_extension("txt"), None);
        assert_eq!(mime_for_extension(""), None);
    }
}
