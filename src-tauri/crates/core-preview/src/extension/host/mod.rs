// 拡張ホスト（設計書 11 章）。拡張フォルダを登録し、拡張子ごとの抽出器を WASM（WASI preview1 のコマンド）で実行する。
//
// - 登録: `register_dir` が `extension.json` を検証し、宣言された wasm をメモリに読んで、コンパイル済みの
//   `Module` を拡張ごとに 1 回だけ作る。拡張フォルダの内容はディスクに展開しない
// - 実行: 呼び出しごとに新しい `Store` を作る。拡張（`Engine`）ごとに実行は 1 つずつ（`gate`）。
//   中断（実時間の上限）は epoch で行うため、同じ `Engine` の別の実行を巻き込まないように直列化する
// - 失敗: 抽出器の異常終了・トラップ・不正な出力は `ExtractError` で返し、アプリは落とさない
// - 権限: 抽出器に与えるのは stdin・stdout・stderr だけ（runtime.rs）

mod manifest;
mod runtime;
#[cfg(test)]
mod tests;

pub use manifest::ManifestError;

use crate::extension::{decode_output, DecodeError, ExtractOutput, OutputKind};
use crate::{OfficeKind, PreviewError, Sheet, Slide};
use manifest::{Manifest, MANIFEST_MAX_BYTES};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wasmtime::{Engine, Module};

/// 拡張マニフェストのファイル名
const MANIFEST_FILE: &str = "extension.json";
/// 抽出器の wasm 1 つの大きさの上限（バイト）。超えるものは登録しない
const WASM_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 実行の上限（設計書 11.1）。既定値は本番の値。テストでは短い時間を差し込む
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// 1 回の抽出の実時間の上限
    pub timeout: Duration,
    /// 線形メモリの上限（バイト）
    pub memory_bytes: usize,
    /// stdin（入力）の上限（バイト）
    pub stdin_bytes: u64,
    /// stdout（出力）の上限（バイト）
    pub stdout_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            memory_bytes: 256 * 1024 * 1024,
            stdin_bytes: 50 * 1024 * 1024,
            stdout_bytes: 16 * 1024 * 1024,
        }
    }
}

/// 登録された拡張の情報（マニフェストの値。画面には出さない）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// 登録された拡張の ID
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionId(String);

impl ExtensionId {
    /// ID の文字列
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 抽出の失敗
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    /// 抽出器が自分で失敗を返した（上限・形式・暗号化・破損。`PreviewError` と同じ種類）
    Failed(PreviewError),
    /// 実時間の上限を超えた
    Timeout,
    /// 線形メモリの上限を超えた
    OutOfMemory,
    /// 入力（stdin）または出力（stdout）がホストの上限を超えた
    TooLarge,
    /// 抽出器が異常終了した、または出力が規約に合わない
    ExtractorFailed,
}

impl ExtractError {
    /// 画面の分類（`PreviewError`）へ写す。時間・メモリ・大きさの超過は「大きすぎる」にまとめる
    pub fn to_preview_error(&self) -> PreviewError {
        match self {
            Self::Failed(err) => *err,
            Self::Timeout | Self::OutOfMemory | Self::TooLarge => PreviewError::TooLarge,
            Self::ExtractorFailed => PreviewError::Corrupt,
        }
    }
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Failed(err) => return write!(f, "extractor reported: {err}"),
            Self::Timeout => "extractor exceeded the time limit",
            Self::OutOfMemory => "extractor exceeded the memory limit",
            Self::TooLarge => "extractor input or output exceeded the size limit",
            Self::ExtractorFailed => "extractor failed",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ExtractError {}

/// 登録済みの抽出器への参照（拡張子から得る）
#[derive(Clone)]
pub struct ExtractorRef {
    extension: Arc<LoadedExtension>,
    index: usize,
}

/// 登録済みの拡張（コンパイル済み）
struct LoadedExtension {
    info: ExtensionInfo,
    engine: Engine,
    /// この拡張の実行を 1 つずつにする（epoch による中断を、別の実行に効かせないため）
    gate: Mutex<()>,
    extractors: Vec<LoadedExtractor>,
}

/// 登録済みの抽出器（コンパイル済み）
struct LoadedExtractor {
    output: OutputKind,
    module: Module,
}

/// 拡張ホスト。アプリの起動時に作り、拡張の一覧を登録して使う
pub struct ExtensionHost {
    limits: RuntimeLimits,
    extensions: Vec<Arc<LoadedExtension>>,
    /// 拡張子（`.` を除いた英小文字）→ 抽出器
    by_file_type: HashMap<String, ExtractorRef>,
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionHost {
    /// 本番の上限で作る
    pub fn new() -> Self {
        Self::with_limits(RuntimeLimits::default())
    }

    /// 上限を指定して作る（テスト用）
    pub fn with_limits(limits: RuntimeLimits) -> Self {
        Self {
            limits,
            extensions: Vec::new(),
            by_file_type: HashMap::new(),
        }
    }

    /// 拡張フォルダを 1 つ登録する（`extension.json` と、宣言された wasm を読む）。
    /// 失敗した拡張は登録されず、ほかの拡張には影響しない
    pub fn register_dir(&mut self, dir: &Path) -> Result<ExtensionId, ManifestError> {
        let bytes = read_limited(&dir.join(MANIFEST_FILE), MANIFEST_MAX_BYTES)?;
        let manifest = manifest::parse(&bytes)?;
        let wasm = manifest
            .extractors
            .iter()
            .map(|x| read_entry(dir, &x.entry))
            .collect::<Result<Vec<_>, _>>()?;
        self.register(manifest, wasm)
    }

    /// 拡張フォルダの一覧を順に登録する。結果は入力と同じ順
    pub fn register_dirs(&mut self, dirs: &[PathBuf]) -> Vec<Result<ExtensionId, ManifestError>> {
        dirs.iter().map(|dir| self.register_dir(dir)).collect()
    }

    /// 登録済みの拡張の情報（登録の順）
    pub fn info(&self) -> Vec<ExtensionInfo> {
        self.extensions.iter().map(|e| e.info.clone()).collect()
    }

    /// 拡張子を扱う抽出器。拡張子は `.` の有無と大文字小文字を問わない（`.DOCX` も `docx` も同じ）
    pub fn extractors_for(&self, file_type: &str) -> Option<ExtractorRef> {
        let key = file_type.strip_prefix('.').unwrap_or(file_type);
        self.by_file_type.get(&key.to_ascii_lowercase()).cloned()
    }

    /// 抽出器を実行し、出力を読み戻す
    pub fn extract(
        &self,
        extractor: &ExtractorRef,
        input: &[u8],
    ) -> Result<ExtractOutput, ExtractError> {
        let loaded = &extractor.extension;
        let decl = &loaded.extractors[extractor.index];
        let stdout = {
            let _gate = runtime::lock(&loaded.gate);
            runtime::run(&loaded.engine, &decl.module, &self.limits, input)?
        };
        decode_output(&stdout, decl.output).map_err(|err| match err {
            // 一部だけの結果は、全体を比べられないため「大きすぎる」として扱う
            DecodeError::Truncated => ExtractError::Failed(PreviewError::TooLarge),
            DecodeError::Malformed => ExtractError::ExtractorFailed,
        })
    }

    /// Office 文書を、拡張子に対応する抽出器で抽出する。抽出器が無ければ `Unsupported`
    pub fn extract_office(
        &self,
        kind: OfficeKind,
        input: &[u8],
    ) -> Result<ExtractOutput, PreviewError> {
        let extractor = self
            .extractors_for(kind.extension())
            .ok_or(PreviewError::Unsupported)?;
        self.extract(&extractor, input)
            .map_err(|err| err.to_preview_error())
    }

    /// docx の段落（抽出器の出力が段落でなければ `Corrupt`。登録時の宣言と違う不正な拡張）
    pub fn docx_paragraphs(&self, input: &[u8]) -> Result<Vec<String>, PreviewError> {
        match self.extract_office(OfficeKind::Docx, input)? {
            ExtractOutput::Paragraphs(paragraphs) => Ok(paragraphs),
            _ => Err(PreviewError::Corrupt),
        }
    }

    /// xlsx のシート（抽出器の出力がセルでなければ `Corrupt`）
    pub fn xlsx_sheets(&self, input: &[u8]) -> Result<Vec<Sheet>, PreviewError> {
        match self.extract_office(OfficeKind::Xlsx, input)? {
            ExtractOutput::Cells(sheets) => Ok(sheets),
            _ => Err(PreviewError::Corrupt),
        }
    }

    /// pptx のスライド（抽出器の出力がページでなければ `Corrupt`）
    pub fn pptx_slides(&self, input: &[u8]) -> Result<Vec<Slide>, PreviewError> {
        match self.extract_office(OfficeKind::Pptx, input)? {
            ExtractOutput::Pages(slides) => Ok(slides),
            _ => Err(PreviewError::Corrupt),
        }
    }

    /// 検証済みのマニフェストと、宣言の順の wasm を登録する（登録の共通部分）
    pub(crate) fn register(
        &mut self,
        manifest: Manifest,
        wasm: Vec<Vec<u8>>,
    ) -> Result<ExtensionId, ManifestError> {
        if self.extensions.iter().any(|e| e.info.id == manifest.id) {
            return Err(ManifestError::DuplicateId);
        }
        let claimed = manifest
            .extractors
            .iter()
            .flat_map(|x| x.file_types.iter())
            .any(|t| self.by_file_type.contains_key(t));
        if claimed {
            return Err(ManifestError::FileTypeConflict);
        }
        if wasm.len() != manifest.extractors.len() {
            return Err(ManifestError::ModuleInvalid);
        }

        // 拡張ごとに 1 つの Engine。モジュールは登録時にコンパイルし、以後は使い回す
        let engine = runtime::new_engine().map_err(|_| ManifestError::ModuleInvalid)?;
        let mut extractors = Vec::with_capacity(wasm.len());
        for (decl, bytes) in manifest.extractors.iter().zip(&wasm) {
            let module =
                runtime::compile(&engine, bytes).map_err(|_| ManifestError::ModuleInvalid)?;
            extractors.push(LoadedExtractor {
                output: decl.output,
                module,
            });
        }

        let info = ExtensionInfo {
            id: manifest.id.clone(),
            name: manifest.name,
            version: manifest.version.to_string(),
        };
        let loaded = Arc::new(LoadedExtension {
            info,
            engine,
            gate: Mutex::new(()),
            extractors,
        });
        for (index, decl) in manifest.extractors.iter().enumerate() {
            for file_type in &decl.file_types {
                let extractor = ExtractorRef {
                    extension: loaded.clone(),
                    index,
                };
                self.by_file_type.insert(file_type.clone(), extractor);
            }
        }
        self.extensions.push(loaded);
        Ok(ExtensionId(manifest.id))
    }
}

/// 上限つきでファイルを読む。上限を超えるものは `TooLarge`（読み切らない）
fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, ManifestError> {
    let file = std::fs::File::open(path).map_err(|_| ManifestError::Unreadable)?;
    let size = file
        .metadata()
        .map_err(|_| ManifestError::Unreadable)?
        .len();
    if size > max {
        return Err(ManifestError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ManifestError::Unreadable)?;
    if bytes.len() as u64 > max {
        return Err(ManifestError::TooLarge);
    }
    Ok(bytes)
}

/// 拡張フォルダ内の entry を読む。実体が拡張フォルダの外（リンクを含む）にあれば読まない
fn read_entry(dir: &Path, entry: &str) -> Result<Vec<u8>, ManifestError> {
    let root = dir
        .canonicalize()
        .map_err(|_| ManifestError::EntryMissing)?;
    let path = root
        .join(entry)
        .canonicalize()
        .map_err(|_| ManifestError::EntryMissing)?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(ManifestError::EntryMissing);
    }
    read_limited(&path, WASM_MAX_BYTES).map_err(|err| match err {
        ManifestError::TooLarge => ManifestError::TooLarge,
        _ => ManifestError::EntryMissing,
    })
}
