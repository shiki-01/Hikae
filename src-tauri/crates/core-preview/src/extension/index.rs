// 拡張の索引（設計書 11.10）の署名検証と検証。ネットワークに依存しない純粋関数だけを置く（取得は呼び出し側）。
//
// - 呼び出しの順序: (1) `verify_index_signature` で索引本文の分離署名を確かめる → 失敗したら一覧を出さない
//   (2) `parse_index` で形と上限を確かめる → (3) 拡張本体の `verify_download` で sha256 と大きさを確かめてから展開する
// - 項目の形・上限が合わない索引は全体を拒否する（署名された索引だけを読むため、不正な項目は作成の誤りとして扱う）
// - 権限が空でない項目と、対応しない apiVersion の項目は、一覧から除外して件数を返す（11.2・11.10）
// - 署名の比較は ed25519-dalek の `verify_strict` に任せる（自前の比較をしない）。sha256 と大きさは公開情報なので通常の比較
// - host の feature に依存しない（WASM 抽出器のビルドにも含まれうるため、wasmtime や tokio は使わない）

use crate::extension::id::valid_id;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;

/// 索引の本文の大きさの上限（バイト）
pub const INDEX_MAX_BYTES: usize = 1024 * 1024;
/// 受け付ける索引の schema の版（この版だけ）
pub const INDEX_SCHEMA: u64 = 1;
/// 索引の項目数の上限
pub const INDEX_MAX_EXTENSIONS: usize = 500;
/// 拡張本体（zip）の大きさの上限（バイト）
pub const INDEX_MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;
/// 分離署名の長さ（ed25519 の署名、バイト）
pub const SIGNATURE_BYTES: usize = 64;

/// 項目の文字列の上限（文字数）。表示のための上限で、設計書 11.10 の「文字列長」にあたる
const NAME_MAX_CHARS: usize = 100;
const SUMMARY_MAX_CHARS: usize = 500;
const VERSION_MAX_CHARS: usize = 64;
const API_VERSION_MAX_CHARS: usize = 8;
const FILE_TYPES_MAX: usize = 16;
const FILE_TYPE_MAX_LEN: usize = 16;
const URL_MAX_BYTES: usize = 2048;
const HTTPS_PREFIX: &str = "https://";

/// 索引の検証の失敗。どの種類も、索引の内容・鍵・署名の値を含まない
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    /// 署名が base64 として読めない
    SignatureEncoding,
    /// 署名を復号した長さが 64 バイトではない
    SignatureLength,
    /// 公開鍵が有効な ed25519 の公開鍵ではない
    InvalidPublicKey,
    /// 署名が索引の本文と一致しない
    SignatureMismatch,
    /// 大きすぎる（索引は 1 MiB まで）
    TooLarge,
    /// JSON として壊れている、または `schema` と `extensions` の形が違う
    Malformed,
    /// `schema` が 1 ではない
    UnsupportedSchema,
    /// 項目数が 500 を超える
    TooManyExtensions,
    /// 項目の形（型・必須の項目）が違う。`index` は `extensions` の先頭からの位置
    ItemShape { index: usize },
    /// 項目の値が上限・形式に合わない。`field` は JSON のキー名（値は含まない）
    InvalidItem { index: usize, field: &'static str },
    /// 同じ `id` の項目が複数ある
    DuplicateId { index: usize },
    /// 拡張本体の大きさが `download.size` と一致しない
    SizeMismatch,
    /// 拡張本体の sha256 が `download.sha256` と一致しない
    HashMismatch,
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignatureEncoding => write!(f, "index signature is not valid base64"),
            Self::SignatureLength => write!(f, "index signature must be 64 bytes"),
            Self::InvalidPublicKey => write!(f, "public key is not a valid ed25519 key"),
            Self::SignatureMismatch => write!(f, "index signature does not match"),
            Self::TooLarge => write!(f, "index exceeds the size limit"),
            Self::Malformed => write!(f, "index is malformed"),
            Self::UnsupportedSchema => write!(f, "index schema is not supported"),
            Self::TooManyExtensions => write!(f, "index has too many extensions"),
            Self::ItemShape { index } => write!(f, "index item {index} has an invalid shape"),
            Self::InvalidItem { index, field } => {
                write!(f, "index item {index} has an invalid {field}")
            }
            Self::DuplicateId { index } => write!(f, "index item {index} has a duplicate id"),
            Self::SizeMismatch => write!(f, "download size does not match the index"),
            Self::HashMismatch => write!(f, "download sha256 does not match the index"),
        }
    }
}

impl std::error::Error for IndexError {}

/// 検証を通った索引。一覧に出してよい項目だけを持つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionIndex {
    /// 一覧に出す項目（索引の順）
    pub extensions: Vec<IndexEntry>,
    /// 権限が空でないため除外した件数（11.2。権限は受け付けない）
    pub excluded_permissions: usize,
    /// 対応しない apiVersion のため除外した件数
    pub excluded_api_version: usize,
}

/// 索引の 1 項目（`permissions` は空のものだけが残る）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub summary: String,
    /// 拡張子（`.` 付き、英小文字と数字）
    pub file_types: Vec<String>,
    pub download: Download,
}

/// 拡張本体の取得先と、展開の前に確かめる値
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// `https://` で始まる URL
    pub url: String,
    /// 小文字 16 進 64 桁
    pub sha256: String,
    /// バイト数（32 MiB 以下）
    pub size: u64,
}

/// 索引本文の分離署名（ed25519、64 バイトを base64）を、公開鍵で確かめる。
///
/// 署名は前後の空白（改行を含む）を除いてから復号する。本文が上限を超えるときは、署名を確かめずに拒否する。
pub fn verify_index_signature(
    index_bytes: &[u8],
    sig_base64: &str,
    public_key: &[u8; 32],
) -> Result<(), IndexError> {
    if index_bytes.len() > INDEX_MAX_BYTES {
        return Err(IndexError::TooLarge);
    }
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_base64.trim())
        .map_err(|_| IndexError::SignatureEncoding)?;
    let sig_array: [u8; SIGNATURE_BYTES] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| IndexError::SignatureLength)?;
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| IndexError::InvalidPublicKey)?;
    let signature = Signature::from_bytes(&sig_array);
    // verify_strict は、弱い鍵と正規でない署名を拒否する
    key.verify_strict(index_bytes, &signature)
        .map_err(|_| IndexError::SignatureMismatch)
}

/// 索引の本文を読み、形と上限を確かめる。
///
/// `supported_api_versions` に含まれない `apiVersion` の項目と、権限が空でない項目は除外して件数を返す。
/// それ以外の不正（形、上限、`https://` 以外の URL、sha256 の形式、重複した id）は索引全体を拒否する。
pub fn parse_index(
    index_bytes: &[u8],
    supported_api_versions: &[&str],
) -> Result<ExtensionIndex, IndexError> {
    if index_bytes.len() > INDEX_MAX_BYTES {
        return Err(IndexError::TooLarge);
    }
    let root: serde_json::Value =
        serde_json::from_slice(index_bytes).map_err(|_| IndexError::Malformed)?;
    let object = root.as_object().ok_or(IndexError::Malformed)?;
    let schema = object.get("schema").ok_or(IndexError::Malformed)?;
    if schema.as_u64() != Some(INDEX_SCHEMA) {
        return Err(IndexError::UnsupportedSchema);
    }
    let items = object
        .get("extensions")
        .and_then(serde_json::Value::as_array)
        .ok_or(IndexError::Malformed)?;
    if items.len() > INDEX_MAX_EXTENSIONS {
        return Err(IndexError::TooManyExtensions);
    }

    let mut extensions = Vec::new();
    let mut excluded_permissions = 0;
    let mut excluded_api_version = 0;
    let mut seen_ids: HashSet<String> = HashSet::new();
    for (index, raw) in items.iter().enumerate() {
        let item: RawItem =
            serde_json::from_value(raw.clone()).map_err(|_| IndexError::ItemShape { index })?;
        let has_permissions = !item.permissions.is_empty();
        let entry = check_item(index, item)?;
        if !seen_ids.insert(entry.id.clone()) {
            return Err(IndexError::DuplicateId { index });
        }

        // 形を確かめたあとで、除外を決める（不正な項目は除外せず拒否する）
        if has_permissions {
            excluded_permissions += 1;
            continue;
        }
        if !supported_api_versions.contains(&entry.api_version.as_str()) {
            excluded_api_version += 1;
            continue;
        }
        extensions.push(entry);
    }

    Ok(ExtensionIndex {
        extensions,
        excluded_permissions,
        excluded_api_version,
    })
}

/// 拡張本体（zip）の大きさと sha256 を、索引の値と照らす。展開の前に呼ぶ。
///
/// 大きさを先に確かめる（sha256 の計算より軽いため）。
pub fn verify_download(
    bytes: &[u8],
    expected_sha256: &str,
    expected_size: u64,
) -> Result<(), IndexError> {
    let actual_size = u64::try_from(bytes.len()).map_err(|_| IndexError::SizeMismatch)?;
    if actual_size != expected_size {
        return Err(IndexError::SizeMismatch);
    }
    let digest = Sha256::digest(bytes);
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected_sha256 {
        return Err(IndexError::HashMismatch);
    }
    Ok(())
}

/// 索引の 1 項目（JSON の形を写したもの）。未知のキーは無視される（serde の既定）
#[derive(Deserialize)]
struct RawItem {
    id: String,
    name: String,
    version: String,
    #[serde(rename = "apiVersion")]
    api_version: String,
    summary: String,
    #[serde(rename = "fileTypes")]
    file_types: Vec<String>,
    download: RawDownload,
    permissions: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawDownload {
    url: String,
    sha256: String,
    size: u64,
}

/// 項目の値の上限と形式を確かめ、`IndexEntry` にする
fn check_item(index: usize, item: RawItem) -> Result<IndexEntry, IndexError> {
    let invalid = |field: &'static str| IndexError::InvalidItem { index, field };

    if !valid_id(&item.id) {
        return Err(invalid("id"));
    }
    if !within_chars(&item.name, NAME_MAX_CHARS) {
        return Err(invalid("name"));
    }
    if !valid_version(&item.version) {
        return Err(invalid("version"));
    }
    if !within_chars(&item.api_version, API_VERSION_MAX_CHARS) {
        return Err(invalid("apiVersion"));
    }
    if !within_chars(&item.summary, SUMMARY_MAX_CHARS) {
        return Err(invalid("summary"));
    }
    if item.file_types.is_empty() || item.file_types.len() > FILE_TYPES_MAX {
        return Err(invalid("fileTypes"));
    }
    if !item.file_types.iter().all(|t| valid_file_type(t)) {
        return Err(invalid("fileTypes"));
    }
    let download = check_download(item.download).map_err(invalid)?;

    Ok(IndexEntry {
        id: item.id,
        name: item.name,
        version: item.version,
        api_version: item.api_version,
        summary: item.summary,
        file_types: item.file_types,
        download,
    })
}

/// 取得先の値を確かめる。失敗のときは、項目の中のキー名を返す
fn check_download(raw: RawDownload) -> Result<Download, &'static str> {
    let url_ok = raw.url.len() <= URL_MAX_BYTES
        && raw.url.starts_with(HTTPS_PREFIX)
        && raw.url.len() > HTTPS_PREFIX.len()
        && raw.url.bytes().all(|b| b.is_ascii_graphic());
    if !url_ok {
        return Err("download.url");
    }
    let sha_ok = raw.sha256.len() == 64
        && raw
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !sha_ok {
        return Err("download.sha256");
    }
    if raw.size == 0 || raw.size > INDEX_MAX_DOWNLOAD_BYTES {
        return Err("download.size");
    }
    Ok(Download {
        url: raw.url,
        sha256: raw.sha256,
        size: raw.size,
    })
}

/// 空でなく、上限の文字数以内
fn within_chars(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max
}

/// semver に近い形（`X.Y.Z`。後ろに `-` か `+` で識別子を付けられる）。ASCII の英数字と `.` `-` `+` だけ
fn valid_version(version: &str) -> bool {
    if version.is_empty() || version.chars().count() > VERSION_MAX_CHARS {
        return false;
    }
    let core_end = version.find(['-', '+']).unwrap_or(version.len());
    let (core, rest) = version.split_at(core_end);
    let parts: Vec<&str> = core.split('.').collect();
    let numeric = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let suffix_ok = rest.is_empty()
        || (rest.len() > 1
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'+'));
    numeric && suffix_ok
}

/// 拡張子（設計書 11.2）: `.` で始まる英小文字と数字の 1〜16 文字
fn valid_file_type(file_type: &str) -> bool {
    let Some(ext) = file_type.strip_prefix('.') else {
        return false;
    };
    (1..=FILE_TYPE_MAX_LEN).contains(&ext.len())
        && ext
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_follow_the_semver_shape() {
        assert!(valid_version("1.0.0"));
        assert!(valid_version("10.2.3-beta.1+build.5"));
        assert!(!valid_version("1.0"));
        assert!(!valid_version("1.0.x"));
        assert!(!valid_version("1..0"));
        assert!(!valid_version("1.0.0-"));
    }

    #[test]
    fn file_types_are_dot_prefixed_lowercase() {
        assert!(valid_file_type(".pdf"));
        assert!(valid_file_type(".mp4"));
        assert!(!valid_file_type("pdf"));
        assert!(!valid_file_type(".PDF"));
        assert!(!valid_file_type("."));
        assert!(!valid_file_type("*"));
        assert!(!valid_file_type(".abcdefghijklmnopq"));
    }
}
