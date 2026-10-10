// 拡張のマニフェスト（`extension.json`、設計書 11.2）の読み込みと検証。
//
// - 64 KiB を超えるものは読まない（`MANIFEST_MAX_BYTES`）
// - 検証に失敗した拡張は登録しない。理由は `ManifestError` の種類だけを返し、パス・内容は含めない
// - 未知のキーは無視する（将来の拡張のため）
// - `permissions` は空配列だけを受け付ける（初期は権限を与えない）
// - `entry` は拡張フォルダ内の相対パスだけ（`..`・絶対パス・`\`・ドライブ指定を拒否）

use crate::extension::OutputKind;
use serde::Deserialize;
use std::fmt;

/// マニフェストの大きさの上限（バイト）
pub(crate) const MANIFEST_MAX_BYTES: u64 = 64 * 1024;
/// 受け付ける API のバージョン（この版だけ）
pub(crate) const API_VERSION: &str = "1";
/// 拡張子の長さの上限（`.` を除く）
const FILE_TYPE_MAX_LEN: usize = 16;
/// `entry` の長さの上限（バイト）
const ENTRY_MAX_LEN: usize = 255;

/// 拡張の登録の失敗の理由。どの種類も、パス・ファイルの内容を含まない
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    /// `extension.json` を読めない（無い、読み取りに失敗した）
    Unreadable,
    /// 大きすぎる（マニフェストは 64 KiB まで。wasm は上限を超える）
    TooLarge,
    /// JSON として壊れている、または必須の項目が無い・型が違う
    Malformed,
    /// `id` の形式が違う（小文字・数字・区切り `.` `-` だけ）
    InvalidId,
    /// `name` が空
    InvalidName,
    /// `version` が semver ではない
    InvalidVersion,
    /// `apiVersion` が対応する版（"1"）ではない
    UnsupportedApiVersion,
    /// `permissions` が空ではない（権限は受け付けない）
    PermissionsNotAllowed,
    /// `output` が `paragraphs`・`cells`・`pages` のどれでもない
    UnknownOutput,
    /// `fileTypes` の形式が違う（`.` で始まる英小文字と数字）、または空
    InvalidFileType,
    /// `entry` が拡張フォルダ内の相対パスではない
    InvalidEntry,
    /// `entry` のファイルが無い、または拡張フォルダの外を指している
    EntryMissing,
    /// `modes` が空、または `view`・`compare` 以外を含む、または重複している
    InvalidMode,
    /// 同じ `id` の拡張がすでに登録されている
    DuplicateId,
    /// 同じ拡張子を、別の抽出器がすでに扱っている
    FileTypeConflict,
    /// wasm を読み込めない（不正なモジュール、エンジンの作成の失敗）
    ModuleInvalid,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Unreadable => "extension manifest is unreadable",
            Self::TooLarge => "extension file exceeds the size limit",
            Self::Malformed => "extension manifest is malformed",
            Self::InvalidId => "extension id is invalid",
            Self::InvalidName => "extension name is empty",
            Self::InvalidVersion => "extension version is not semver",
            Self::UnsupportedApiVersion => "extension apiVersion is not supported",
            Self::PermissionsNotAllowed => "extension permissions are not allowed",
            Self::UnknownOutput => "extractor output kind is unknown",
            Self::InvalidFileType => "file type is invalid",
            Self::InvalidEntry => "entry path is invalid",
            Self::EntryMissing => "entry file is missing",
            Self::InvalidMode => "renderer mode is invalid",
            Self::DuplicateId => "extension id is already registered",
            Self::FileTypeConflict => "file type is already handled by another extractor",
            Self::ModuleInvalid => "extension module could not be loaded",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ManifestError {}

/// 検証済みのマニフェスト
#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: semver::Version,
    /// 表示の宣言（拡張子 → iframe の入口）
    pub(crate) renderers: Vec<RendererDecl>,
    pub(crate) extractors: Vec<ExtractorDecl>,
}

/// 表示の方法（設計書 11.2 の `modes`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderMode {
    /// 1 時点の表示
    View,
    /// 2 時点の比較表示（第 3 段階で使う。今は宣言の検証だけ）
    Compare,
}

impl RenderMode {
    /// マニフェスト・画面の表記（`view` / `compare`）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Compare => "compare",
        }
    }
}

/// 表示の宣言
#[derive(Debug, Clone)]
pub(crate) struct RendererDecl {
    /// 拡張子（`.` を除いた英小文字）。全部の拡張子を表す `*` は、この 1 件だけで使う
    pub(crate) file_types: Vec<String>,
    /// 対応する表示の方法（空は不可）
    pub(crate) modes: Vec<RenderMode>,
    /// 拡張フォルダ内の相対パス（iframe の入口。例: `ui/index.html`）
    pub(crate) entry: String,
}

/// 抽出器の宣言
#[derive(Debug, Clone)]
pub(crate) struct ExtractorDecl {
    /// 拡張子（`.` を除いた英小文字）
    pub(crate) file_types: Vec<String>,
    pub(crate) output: OutputKind,
    /// 拡張フォルダ内の相対パス（wasm）
    pub(crate) entry: String,
}

// ---- JSON の形（検証の前の生の値） ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    api_version: String,
    #[serde(default)]
    contributes: RawContributes,
    /// 空配列だけを受け付ける。型は配列であることだけを確かめる
    #[serde(default)]
    permissions: Vec<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawContributes {
    #[serde(default)]
    renderers: Vec<RawRenderer>,
    #[serde(default)]
    extractors: Vec<RawExtractor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRenderer {
    file_types: Vec<String>,
    modes: Vec<String>,
    entry: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExtractor {
    file_types: Vec<String>,
    output: String,
    entry: String,
}

/// マニフェストを読み、検証する
pub(crate) fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    if bytes.len() as u64 > MANIFEST_MAX_BYTES {
        return Err(ManifestError::TooLarge);
    }
    let raw: RawManifest = serde_json::from_slice(bytes).map_err(|_| ManifestError::Malformed)?;
    if !valid_id(&raw.id) {
        return Err(ManifestError::InvalidId);
    }
    if raw.name.trim().is_empty() {
        return Err(ManifestError::InvalidName);
    }
    let version =
        semver::Version::parse(&raw.version).map_err(|_| ManifestError::InvalidVersion)?;
    if raw.api_version != API_VERSION {
        return Err(ManifestError::UnsupportedApiVersion);
    }
    if !raw.permissions.is_empty() {
        return Err(ManifestError::PermissionsNotAllowed);
    }

    let renderers = raw
        .contributes
        .renderers
        .into_iter()
        .map(|r| {
            validate_entry(&r.entry)?;
            Ok(RendererDecl {
                file_types: normalize_renderer_file_types(&r.file_types)?,
                modes: parse_modes(&r.modes)?,
                entry: r.entry,
            })
        })
        .collect::<Result<Vec<_>, ManifestError>>()?;

    let mut extractors: Vec<ExtractorDecl> = Vec::new();
    for x in raw.contributes.extractors {
        validate_entry(&x.entry)?;
        let output = OutputKind::parse(&x.output).ok_or(ManifestError::UnknownOutput)?;
        let file_types = normalize_file_types(&x.file_types)?;
        // 同じ拡張子を 2 つの抽出器が扱うと、どちらを使うか決められないため拒否する
        let conflict = extractors
            .iter()
            .flat_map(|e| e.file_types.iter())
            .any(|t| file_types.contains(t));
        if conflict {
            return Err(ManifestError::FileTypeConflict);
        }
        extractors.push(ExtractorDecl {
            file_types,
            output,
            entry: x.entry,
        });
    }

    Ok(Manifest {
        id: raw.id,
        name: raw.name,
        version,
        renderers,
        extractors,
    })
}

/// `id` の形式（設計書 11.2）: `^[a-z0-9]+([.-][a-z0-9]+)*$`
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    if !alnum(first) || !alnum(last) {
        return false;
    }
    // 区切りの直後は英数字でなければならない（連続した区切りを拒否する）
    let mut after_separator = false;
    for &c in bytes {
        if alnum(c) {
            after_separator = false;
        } else if c == b'.' || c == b'-' {
            if after_separator {
                return false;
            }
            after_separator = true;
        } else {
            return false;
        }
    }
    true
}

/// `fileTypes` を拡張子（`.` を除いた英小文字）にする。形式が違えば拒否する
fn normalize_file_types(types: &[String]) -> Result<Vec<String>, ManifestError> {
    if types.is_empty() {
        return Err(ManifestError::InvalidFileType);
    }
    types
        .iter()
        .map(|t| {
            let ext = t.strip_prefix('.').ok_or(ManifestError::InvalidFileType)?;
            let valid = (1..=FILE_TYPE_MAX_LEN).contains(&ext.len())
                && ext
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            if valid {
                Ok(ext.to_owned())
            } else {
                Err(ManifestError::InvalidFileType)
            }
        })
        .collect()
}

/// 表示の `fileTypes`。`["*"]`（全部の拡張子。最も低い優先度）だけを受け付け、`*` を他と並べることは拒否する
fn normalize_renderer_file_types(types: &[String]) -> Result<Vec<String>, ManifestError> {
    if types.len() == 1 && types[0] == "*" {
        return Ok(vec!["*".to_owned()]);
    }
    if types.iter().any(|t| t == "*") {
        return Err(ManifestError::InvalidFileType);
    }
    normalize_file_types(types)
}

/// `modes`（`view`・`compare` のみ。空と重複は拒否する）
fn parse_modes(modes: &[String]) -> Result<Vec<RenderMode>, ManifestError> {
    if modes.is_empty() {
        return Err(ManifestError::InvalidMode);
    }
    let mut parsed: Vec<RenderMode> = Vec::with_capacity(modes.len());
    for mode in modes {
        let value = match mode.as_str() {
            "view" => RenderMode::View,
            "compare" => RenderMode::Compare,
            _ => return Err(ManifestError::InvalidMode),
        };
        if parsed.contains(&value) {
            return Err(ManifestError::InvalidMode);
        }
        parsed.push(value);
    }
    Ok(parsed)
}

/// `entry` の形式の検査（拡張フォルダ内の相対パスだけを通す）。ファイルの有無は登録時に確かめる
pub(crate) fn validate_entry(entry: &str) -> Result<(), ManifestError> {
    if entry.is_empty() || entry.len() > ENTRY_MAX_LEN {
        return Err(ManifestError::InvalidEntry);
    }
    // バックスラッシュ（Windows の区切り）、ドライブ指定（`C:`）、NUL を含むものは拒否する
    if entry.contains(['\\', ':', '\0']) || entry.starts_with('/') {
        return Err(ManifestError::InvalidEntry);
    }
    let escapes = entry
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..");
    if escapes {
        return Err(ManifestError::InvalidEntry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小の正しいマニフェスト（抽出器 1 つ）
    fn valid_json() -> String {
        r#"{
  "id": "example.office-test",
  "name": "テスト",
  "version": "1.2.3",
  "apiVersion": "1",
  "contributes": {
    "renderers": [],
    "extractors": [{ "fileTypes": [".docx"], "output": "paragraphs", "entry": "extract.wasm" }]
  },
  "permissions": []
}"#
        .to_owned()
    }

    fn with(field: &str, value: &str) -> String {
        // 1 つの項目だけを差し替えた JSON を作る（テストの読みやすさのため）
        let mut json: serde_json::Value = serde_json::from_str(&valid_json()).unwrap_or_default();
        json[field] = serde_json::from_str(value).unwrap_or(serde_json::Value::Null);
        json.to_string()
    }

    #[test]
    fn a_valid_manifest_is_accepted_and_file_types_are_normalized() -> Result<(), ManifestError> {
        let m = parse(valid_json().as_bytes())?;
        assert_eq!(m.id, "example.office-test");
        assert_eq!(m.version, semver::Version::new(1, 2, 3));
        assert_eq!(m.extractors.len(), 1);
        assert_eq!(m.extractors[0].file_types, vec!["docx".to_owned()]);
        assert_eq!(m.extractors[0].output, OutputKind::Paragraphs);
        Ok(())
    }

    /// 表示の宣言 1 件を含むマニフェスト（`fileTypes`・`modes`・`entry` を差し替えて使う）
    fn renderer_json(file_types: &str, modes: &str, entry: &str) -> String {
        format!(
            r#"{{"id":"test.renderer","name":"x","version":"1.0.0","apiVersion":"1",
                "contributes":{{"renderers":[{{"fileTypes":{file_types},"modes":{modes},"entry":"{entry}"}}]}}}}"#
        )
    }

    #[test]
    fn a_renderer_with_only_the_star_file_type_is_accepted() -> Result<(), ManifestError> {
        let m = parse(renderer_json(r#"["*"]"#, r#"["view"]"#, "ui/index.html").as_bytes())?;
        assert_eq!(m.renderers[0].file_types, vec!["*".to_owned()]);
        assert_eq!(m.renderers[0].modes, vec![RenderMode::View]);
        let m = parse(
            renderer_json(
                r#"[".png", ".jpg"]"#,
                r#"["view","compare"]"#,
                "ui/index.html",
            )
            .as_bytes(),
        )?;
        assert_eq!(
            m.renderers[0].file_types,
            vec!["png".to_owned(), "jpg".to_owned()]
        );
        assert_eq!(
            m.renderers[0].modes,
            vec![RenderMode::View, RenderMode::Compare]
        );
        Ok(())
    }

    #[test]
    fn star_is_only_accepted_alone() {
        for types in [
            r#"["*.png"]"#,
            r#"["**"]"#,
            r#"["*", ".png"]"#,
            r#"["*", "*"]"#,
            r#"[]"#,
        ] {
            assert_eq!(
                parse(renderer_json(types, r#"["view"]"#, "ui/index.html").as_bytes()).err(),
                Some(ManifestError::InvalidFileType),
                "fileTypes {types} should be rejected"
            );
        }
    }

    #[test]
    fn modes_must_be_non_empty_known_and_unique() {
        for modes in [r#"[]"#, r#"["edit"]"#, r#"["view","view"]"#, r#"["VIEW"]"#] {
            assert_eq!(
                parse(renderer_json(r#"[".png"]"#, modes, "ui/index.html").as_bytes()).err(),
                Some(ManifestError::InvalidMode),
                "modes {modes} should be rejected"
            );
        }
    }

    #[test]
    fn a_renderer_entry_with_traversal_is_rejected() {
        assert_eq!(
            parse(renderer_json(r#"["*"]"#, r#"["view"]"#, "../ui/index.html").as_bytes()).err(),
            Some(ManifestError::InvalidEntry)
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let json = valid_json().replace(
            r#""permissions": []"#,
            r#""permissions": [], "futureKey": {"x": 1}, "modes": ["view"]"#,
        );
        assert!(parse(json.as_bytes()).is_ok());
    }

    #[test]
    fn invalid_ids_are_rejected() {
        for id in ["", "Bad", "a..b", "a.-b", "-a", "a-", "a_b", "a b", "a/b"] {
            let json = with("id", &format!("\"{id}\""));
            assert_eq!(
                parse(json.as_bytes()).err(),
                Some(ManifestError::InvalidId),
                "id {id:?} should be rejected"
            );
        }
        for id in ["a", "a.b", "a-b.c-d1", "hikae.builtin.office"] {
            let json = with("id", &format!("\"{id}\""));
            assert!(
                parse(json.as_bytes()).is_ok(),
                "id {id:?} should be accepted"
            );
        }
    }

    #[test]
    fn api_version_must_be_one() {
        let json = with("apiVersion", "\"2\"");
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::UnsupportedApiVersion)
        );
    }

    #[test]
    fn non_empty_permissions_are_rejected() {
        let json = with("permissions", r#"["network"]"#);
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::PermissionsNotAllowed)
        );
        // 空配列でない形（文字列）は型の違いとして拒否する
        let json = with("permissions", r#""network""#);
        assert_eq!(parse(json.as_bytes()).err(), Some(ManifestError::Malformed));
    }

    #[test]
    fn version_must_be_semver() {
        let json = with("version", "\"1.0\"");
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::InvalidVersion)
        );
    }

    #[test]
    fn entries_must_stay_inside_the_extension_folder() {
        for entry in [
            "../x.wasm",
            "a/../../x.wasm",
            "/abs.wasm",
            "C:x.wasm",
            "C:/x.wasm",
            "a\\b.wasm",
            "./x.wasm",
            "a//b.wasm",
            "",
        ] {
            assert_eq!(
                validate_entry(entry),
                Err(ManifestError::InvalidEntry),
                "entry {entry:?} should be rejected"
            );
        }
        assert!(validate_entry("extract.wasm").is_ok());
        assert!(validate_entry("ui/index.html").is_ok());
    }

    #[test]
    fn an_entry_with_traversal_in_an_extractor_is_rejected() {
        let json = valid_json().replace("extract.wasm", "../../escape.wasm");
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::InvalidEntry)
        );
    }

    #[test]
    fn unknown_output_and_bad_file_types_are_rejected() {
        let json = valid_json().replace(r#""paragraphs""#, r#""images""#);
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::UnknownOutput)
        );
        for types in [r#"["docx"]"#, r#"[]"#, r#"[".DOCX"]"#, r#"["."]"#] {
            let json = valid_json().replace(r#"[".docx"]"#, types);
            assert_eq!(
                parse(json.as_bytes()).err(),
                Some(ManifestError::InvalidFileType),
                "types {types} should be rejected"
            );
        }
    }

    #[test]
    fn two_extractors_for_one_file_type_conflict() {
        let json = valid_json().replace(
            r#"{ "fileTypes": [".docx"], "output": "paragraphs", "entry": "extract.wasm" }"#,
            r#"{ "fileTypes": [".docx"], "output": "paragraphs", "entry": "a.wasm" },
               { "fileTypes": [".docx"], "output": "paragraphs", "entry": "b.wasm" }"#,
        );
        assert_eq!(
            parse(json.as_bytes()).err(),
            Some(ManifestError::FileTypeConflict)
        );
    }

    #[test]
    fn a_manifest_over_64_kib_is_too_large() {
        let mut bytes = valid_json().into_bytes();
        bytes.resize(MANIFEST_MAX_BYTES as usize + 1, b' ');
        assert_eq!(parse(&bytes).err(), Some(ManifestError::TooLarge));
    }

    #[test]
    fn broken_json_and_missing_required_keys_are_malformed() {
        assert_eq!(parse(b"{").err(), Some(ManifestError::Malformed));
        assert_eq!(parse(b"{}").err(), Some(ManifestError::Malformed));
    }
}
