// モデルカタログ（設計 10.2、10.5）。既定は埋め込みの JSON を読む。構造は外部 JSON に差し替え可能な形（serde）。
// 読み込み時に検証する（ID の重複、安全な名前、64 桁の SHA-256、許可された取得元の https URL など）。

use crate::download::{is_allowed_url, is_safe_component};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// 同梱のカタログ（既定）
const BUILTIN_CATALOG_JSON: &str = include_str!("catalog.json");

/// 読み取れるカタログの形式バージョン
const SUPPORTED_VERSION: u32 = 1;

/// カタログの 1 項目（1 モデルの 1 量子化版）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// モデルの識別子（保存先フォルダ名になる）
    pub id: String,
    /// 表示名（UI の文言キーに変換する前の既定値）
    pub display_name: String,
    /// 段階（設計 10.2）。1〜4
    pub stage: u8,
    /// 保存するファイル名
    pub file_name: String,
    /// 取得元の URL（https、許可されたホストに限る）
    pub url: String,
    /// ファイルの SHA-256（小文字の 16 進 64 桁）
    pub sha256: String,
    /// ファイルのサイズ（バイト）
    pub size_bytes: u64,
    /// ライセンス名
    pub license_name: String,
    /// ライセンス本文の URL
    pub license_url: String,
    /// 必要メモリの目安（OS が報告する総メモリのバイト数）。
    /// 8GB の PC は OS の報告値が 8GiB を下回るため、7GiB を下限にしている
    pub min_total_memory_bytes: u64,
    /// 同じ段階での推奨
    pub recommended: bool,
}

/// カタログ全体
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<CatalogEntry>,
}

/// カタログの読み込み・検証のエラー。内部の診断用で、利用者には固定文言で返す。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CatalogError {
    #[error("カタログの JSON を読めません")]
    Parse,
    #[error("カタログの形式バージョンが対応外です")]
    UnsupportedVersion,
    #[error("カタログの項目が正しくありません: {0}")]
    InvalidEntry(&'static str),
}

impl Catalog {
    /// 同梱の既定カタログを読む。
    pub fn builtin() -> Result<Self, CatalogError> {
        Self::parse(BUILTIN_CATALOG_JSON)
    }

    /// JSON からカタログを読み、検証する。
    pub fn parse(json: &str) -> Result<Self, CatalogError> {
        let catalog: Catalog = serde_json::from_str(json).map_err(|_| CatalogError::Parse)?;
        catalog.validate()?;
        Ok(catalog)
    }

    /// ID で項目を探す。
    pub fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    /// 段階が一致する項目を返す。
    pub fn stage(&self, stage: u8) -> impl Iterator<Item = &CatalogEntry> {
        self.models.iter().filter(move |m| m.stage == stage)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.version != SUPPORTED_VERSION {
            return Err(CatalogError::UnsupportedVersion);
        }
        let mut ids = HashSet::new();
        for entry in &self.models {
            validate_entry(entry)?;
            if !ids.insert(entry.id.as_str()) {
                return Err(CatalogError::InvalidEntry("id が重複しています"));
            }
        }
        Ok(())
    }
}

fn validate_entry(entry: &CatalogEntry) -> Result<(), CatalogError> {
    if !is_safe_component(&entry.id) {
        return Err(CatalogError::InvalidEntry("id が安全な名前ではありません"));
    }
    if !is_safe_component(&entry.file_name) {
        return Err(CatalogError::InvalidEntry(
            "file_name が安全な名前ではありません",
        ));
    }
    if !(1..=4).contains(&entry.stage) {
        return Err(CatalogError::InvalidEntry("stage が 1〜4 の範囲外です"));
    }
    if entry.size_bytes == 0 {
        return Err(CatalogError::InvalidEntry("size_bytes が 0 です"));
    }
    if entry.sha256.len() != 64
        || !entry
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CatalogError::InvalidEntry(
            "sha256 が小文字の 16 進 64 桁ではありません",
        ));
    }
    let url =
        Url::parse(&entry.url).map_err(|_| CatalogError::InvalidEntry("url を解析できません"))?;
    if !is_allowed_url(&url) {
        return Err(CatalogError::InvalidEntry(
            "url が許可された取得元ではありません",
        ));
    }
    if entry.license_name.trim().is_empty() {
        return Err(CatalogError::InvalidEntry("license_name が空です"));
    }
    let license = Url::parse(&entry.license_url)
        .map_err(|_| CatalogError::InvalidEntry("license_url を解析できません"))?;
    if license.scheme() != "https" {
        return Err(CatalogError::InvalidEntry(
            "license_url が https ではありません",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid() {
        let catalog = Catalog::builtin().unwrap();
        assert_eq!(catalog.models.len(), 4);
        let gemma = catalog.get("gemma-4-e2b-it-q4_k_m").unwrap();
        assert!(gemma.recommended);
        assert_eq!(gemma.stage, 2);
        assert_eq!(gemma.size_bytes, 3_106_738_272);
        // 段階 2 は E2B・Qwen3-4B・E4B の 3 つ（E4B は評価ノート 11.3。既定の推奨は E2B のまま）
        assert_eq!(catalog.stage(2).count(), 3);
        let e4b = catalog.get("gemma-4-e4b-it-q4_k_m").unwrap();
        assert_eq!(e4b.stage, 2);
        assert!(!e4b.recommended);
        assert_eq!(e4b.size_bytes, 4_977_171_584);
        // 必要メモリは 12GiB（8GB の PC では導入できず、理由つきで灰色になる。評価ノート 15 節の実測ピーク 4.2GB）
        assert_eq!(e4b.min_total_memory_bytes, 12 * 1024 * 1024 * 1024);
        // 段階 1 は Qwen3.5-2B（設計 10.2。評価ノート 11.3）。既定は段階 2 の Gemma 4 E2B のまま
        assert_eq!(catalog.stage(1).count(), 1);
        let light = catalog.get("qwen3.5-2b-q4_k_m").unwrap();
        assert_eq!(light.stage, 1);
        assert!(!light.recommended);
        assert_eq!(light.size_bytes, 1_280_835_840);
    }

    #[test]
    fn builtin_sha256_values_match_evaluation_report() {
        let catalog = Catalog::builtin().unwrap();
        assert_eq!(
            catalog.get("gemma-4-e2b-it-q4_k_m").unwrap().sha256,
            "740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8"
        );
        assert_eq!(
            catalog.get("qwen3-4b-instruct-2507-q4_k_m").unwrap().sha256,
            "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597"
        );
        assert_eq!(
            catalog.get("qwen3.5-2b-q4_k_m").unwrap().sha256,
            "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223"
        );
        assert_eq!(
            catalog.get("gemma-4-e4b-it-q4_k_m").unwrap().sha256,
            "85a896a047553e842f25297ee5b031d64ff30147d9c4af17b1e4b394cd1fab87"
        );
    }

    #[test]
    fn builtin_ids_and_urls_are_unique_and_well_formed() {
        // ID の重複は読み込みで拒否される。URL は取得元の形式（resolve/main）と SHA-256 の 64 桁を確かめる
        let catalog = Catalog::builtin().unwrap();
        let mut urls = HashSet::new();
        let mut hashes = HashSet::new();
        for m in &catalog.models {
            assert!(urls.insert(m.url.as_str()), "{}", m.id);
            assert!(hashes.insert(m.sha256.as_str()), "{}", m.id);
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.url.contains("/resolve/main/"), "{}", m.id);
            assert!(m.size_bytes > 0, "{}", m.id);
        }
    }

    #[test]
    fn builtin_entries_use_allowed_https_urls() {
        let catalog = Catalog::builtin().unwrap();
        for m in &catalog.models {
            let url = Url::parse(&m.url).unwrap();
            assert!(is_allowed_url(&url), "{}", m.id);
            assert!(m.url.ends_with(&format!("/resolve/main/{}", m.file_name)));
        }
    }

    /// 同梱カタログの原文（検証テストで一部を書き換えて使う）
    fn valid_json() -> String {
        BUILTIN_CATALOG_JSON.to_string()
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = valid_json();
        let dup = json.replace("qwen3-4b-instruct-2507-q4_k_m", "gemma-4-e2b-it-q4_k_m");
        assert!(matches!(
            Catalog::parse(&dup),
            Err(CatalogError::InvalidEntry(_))
        ));
    }

    #[test]
    fn rejects_bad_sha256() {
        let json = valid_json();
        let short = json.replace(
            "740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8",
            "740185b2",
        );
        assert!(Catalog::parse(&short).is_err());
        let upper = json.replace(
            "740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8",
            "740185B21D22CEB83A11C3AA62AD5842EF32C70F6096D756BBEE85A1E4EC34B8",
        );
        assert!(Catalog::parse(&upper).is_err());
    }

    #[test]
    fn rejects_disallowed_urls() {
        let json = valid_json();
        let http = json.replace(
            "https://huggingface.co/unsloth/gemma",
            "http://huggingface.co/unsloth/gemma",
        );
        assert!(Catalog::parse(&http).is_err());
        let other = json.replace(
            "https://huggingface.co/unsloth/gemma",
            "https://example.com/gemma",
        );
        assert!(Catalog::parse(&other).is_err());
    }

    #[test]
    fn rejects_traversal_in_id_and_file_name() {
        let json = valid_json();
        let id = json.replace("\"id\": \"gemma-4-e2b-it-q4_k_m\"", "\"id\": \"../x\"");
        assert!(Catalog::parse(&id).is_err());
        let file = json.replace(
            "\"file_name\": \"gemma-4-E2B-it-Q4_K_M.gguf\"",
            "\"file_name\": \"..\\\\x.gguf\"",
        );
        assert!(Catalog::parse(&file).is_err());
    }

    #[test]
    fn rejects_unknown_version_and_bad_json() {
        let json = valid_json();
        let v2 = json.replace("\"version\": 1", "\"version\": 2");
        assert_eq!(Catalog::parse(&v2), Err(CatalogError::UnsupportedVersion));
        assert_eq!(Catalog::parse("{"), Err(CatalogError::Parse));
    }
}
