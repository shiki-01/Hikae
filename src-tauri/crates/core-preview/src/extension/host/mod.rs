// 拡張ホスト（設計書 11 章）。拡張フォルダを登録し、拡張子ごとの抽出器を WASM（WASI preview1 のコマンド）で実行する。
//
// - 登録: `register_dir` が `extension.json` を検証し、宣言された wasm をメモリに読んで、コンパイル済みの
//   `Module` を拡張ごとに 1 回だけ作る。拡張フォルダの内容はディスクに展開しない
// - 実行: 呼び出しごとに新しい `Store` を作る。拡張（`Engine`）ごとに実行は 1 つずつ（`gate`）。
//   中断（実時間の上限）は epoch で行うため、同じ `Engine` の別の実行を巻き込まないように直列化する
// - 失敗: 抽出器の異常終了・トラップ・不正な出力は `ExtractError` で返し、アプリは落とさない
// - 権限: 抽出器に与えるのは stdin・stdout・stderr だけ（runtime.rs）

mod manifest;
#[cfg(test)]
mod renderer_tests;
mod runtime;
#[cfg(test)]
mod tests;

pub use manifest::{ManifestError, RenderMode};

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

/// 登録済みの表示（renderer）の宣言。拡張の入口（iframe に読ませる HTML）を指す
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererInfo {
    /// 登録した拡張の ID
    pub extension_id: String,
    /// 拡張フォルダ内の相対パス（例: `ui/index.html`）
    pub entry: String,
    /// 拡張子（`.` を除いた英小文字）。全部の拡張子を表す場合は `*` の 1 件だけ
    pub file_types: Vec<String>,
    /// 対応する表示の方法
    pub modes: Vec<RenderMode>,
}

/// 登録済みの拡張（コンパイル済み）
struct LoadedExtension {
    info: ExtensionInfo,
    engine: Engine,
    /// この拡張の実行を 1 つずつにする（epoch による中断を、別の実行に効かせないため）
    gate: Mutex<()>,
    extractors: Vec<LoadedExtractor>,
    /// 拡張フォルダの実体（正規化済み）。`ext://` の配信で使う。メモリだけで登録したものは None
    root: Option<PathBuf>,
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
    /// 登録済みの表示の宣言（登録の順）
    renderers: Vec<RendererInfo>,
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
            renderers: Vec::new(),
        }
    }

    /// 拡張フォルダを 1 つ登録する（`extension.json` と、宣言された wasm・表示の入口を確かめる）。
    /// 失敗した拡張は登録されず、ほかの拡張には影響しない
    pub fn register_dir(&mut self, dir: &Path) -> Result<ExtensionId, ManifestError> {
        let bytes = read_limited(&dir.join(MANIFEST_FILE), MANIFEST_MAX_BYTES)?;
        let manifest = manifest::parse(&bytes)?;
        let root = dir.canonicalize().map_err(|_| ManifestError::Unreadable)?;
        for renderer in &manifest.renderers {
            check_entry(&root, &renderer.entry)?;
        }
        let wasm = manifest
            .extractors
            .iter()
            .map(|x| read_entry(&root, &x.entry))
            .collect::<Result<Vec<_>, _>>()?;
        self.register_in(manifest, wasm, Some(root))
    }

    /// 登録済みの表示の宣言（登録の順）
    pub fn renderers(&self) -> &[RendererInfo] {
        &self.renderers
    }

    /// 拡張子と表示の方法から、表示を決める。同じ拡張子の宣言があればそれを、無ければ `*` を使う。
    /// 拡張子は `.` の有無と大文字小文字を問わない
    pub fn renderer_for(&self, file_type: &str, mode: RenderMode) -> Option<&RendererInfo> {
        let key = file_type
            .strip_prefix('.')
            .unwrap_or(file_type)
            .to_ascii_lowercase();
        let find = |wanted: &str| {
            self.renderers
                .iter()
                .find(|r| r.modes.contains(&mode) && r.file_types.iter().any(|t| t == wanted))
        };
        find(&key).or_else(|| find("*"))
    }

    /// 登録済みの拡張のフォルダ（正規化済み）。登録されていなければ None。`ext://` の配信の根
    pub fn extension_root(&self, id: &str) -> Option<PathBuf> {
        self.extensions
            .iter()
            .find(|e| e.info.id == id)
            .and_then(|e| e.root.clone())
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

    /// 検証済みのマニフェストと、宣言の順の wasm をメモリだけから登録する（テスト用）
    #[cfg(test)]
    pub(crate) fn register(
        &mut self,
        manifest: Manifest,
        wasm: Vec<Vec<u8>>,
    ) -> Result<ExtensionId, ManifestError> {
        self.register_in(manifest, wasm, None)
    }

    /// 検証済みのマニフェストと、宣言の順の wasm を登録する（登録の共通部分）。
    /// `root` は拡張フォルダの実体（正規化済み）。失敗したときは何も変えない
    pub(crate) fn register_in(
        &mut self,
        manifest: Manifest,
        wasm: Vec<Vec<u8>>,
        root: Option<PathBuf>,
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
        // 表示は、同じ拡張子・同じ方法を 2 つの宣言が扱うと選べないため、重複を拒否する（`*` も同じ扱い）
        let mut claims: Vec<(&str, RenderMode)> = Vec::new();
        for decl in &manifest.renderers {
            for file_type in &decl.file_types {
                for &mode in &decl.modes {
                    let taken = claims.contains(&(file_type.as_str(), mode))
                        || self
                            .renderers
                            .iter()
                            .any(|r| r.modes.contains(&mode) && r.file_types.contains(file_type));
                    if taken {
                        return Err(ManifestError::FileTypeConflict);
                    }
                    claims.push((file_type.as_str(), mode));
                }
            }
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
            name: manifest.name.clone(),
            version: manifest.version.to_string(),
        };
        for decl in &manifest.renderers {
            self.renderers.push(RendererInfo {
                extension_id: manifest.id.clone(),
                entry: decl.entry.clone(),
                file_types: decl.file_types.clone(),
                modes: decl.modes.clone(),
            });
        }
        let loaded = Arc::new(LoadedExtension {
            info,
            engine,
            gate: Mutex::new(()),
            extractors,
            root,
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

/// 拡張フォルダ内の entry の実体を確かめる（`root` は正規化済み）。
/// 実体が拡張フォルダの外（リンクを含む）にあるもの、通常のファイルでないものは `EntryMissing`
fn check_entry(root: &Path, entry: &str) -> Result<PathBuf, ManifestError> {
    let path = root
        .join(entry)
        .canonicalize()
        .map_err(|_| ManifestError::EntryMissing)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(ManifestError::EntryMissing);
    }
    Ok(path)
}

/// 拡張フォルダ内の entry（wasm）を読む。実体の確認は `check_entry` と同じ
fn read_entry(root: &Path, entry: &str) -> Result<Vec<u8>, ManifestError> {
    let path = check_entry(root, entry)?;
    read_limited(&path, WASM_MAX_BYTES).map_err(|err| match err {
        ManifestError::TooLarge => ManifestError::TooLarge,
        _ => ManifestError::EntryMissing,
    })
}
