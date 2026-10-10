// ファイルのプレビュー（「すべてのファイル」で選んだファイルの内容を表示する）。1 つの版の内容を種類ごとに返す。
// 2 つの版を比べる処理ではない（比較は `diff`・`compare_office`）。読み取りのみで、直列キューは通さない（`run_blocking`）。
//
// - 種類は拡張子で決める。docx・xlsx・pptx は抽出して返す。画像と pdf は UI が `app-blob` で読むため、
//   ここでは内容を返さず `binary` にする（存在しなければ `not-found`）
// - それ以外は UTF-8 のテキストとして読む。読めなければ、上限を超えるものは `too-large`、そうでなければ `binary`
// - 抽出の失敗はエラーにせず、`unavailable` で理由を返す
// - 時点は画面の ID（`current`・`work`・`this`・`cloud`・短縮 ID）。版が無いときは `not-found`
// - 表示の上限を超えた分は `truncated` で示す（段落 5,000、スライド 300、シート 1 枚あたりのセル 20,000）。
//   シートが 50 を超えるブックと、1 MiB を超えるテキストは一部だけ出すと内容を誤解させるため、`too-large` にする
// - ファイルの内容・パスは、エラーにも `technical_info` にも出さない

use crate::{git_runner, load_project, run_blocking, version, AppError, AppState};
use core_ops::{OfficeFile, Ops, OpsError, TextFile};
use core_preview::{
    extract_docx, extract_pptx, extract_xlsx, office_kind, Limits, OfficeKind, PreviewError, Sheet,
    Slide,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// テキストとして読む上限（1 MiB）。これを超えるテキストは `too-large`
const TEXT_MAX_BYTES: u64 = 1024 * 1024;
/// 画面に返す段落の上限（docx）
const MAX_PARAGRAPHS: usize = 5_000;
/// 画面に返すスライドの上限（pptx）
const MAX_SLIDES: usize = 300;
/// 画面に返すシートの上限（xlsx）。超えるブックは `too-large`
const MAX_SHEETS: usize = 50;
/// 画面に返すセルの上限（シート 1 枚あたり）
const MAX_CELLS_PER_SHEET: usize = 20_000;

/// 抽出できなかった理由（画面は短い文言を出す）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewUnavailableReason {
    /// 上限（ファイルの大きさ・件数・段落など）を超えた
    TooLarge,
    /// 対応していない形式（拡張子と中身が合わない、旧形式、暗号化を含む）
    UnsupportedFormat,
    /// 壊れている
    Corrupt,
    /// バイナリ（テキストとして表示できない）
    Binary,
}

/// xlsx のシート 1 枚。`cells` は値のあるセルだけ（行 → 列の順）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct XlsxSheet {
    pub name: String,
    /// A1 から最後の値までの行数
    pub rows: u32,
    /// A1 から最後の値までの列数
    pub cols: u32,
    pub cells: Vec<XlsxCell>,
    /// `cells` に入らず打ち切ったか（シート 1 枚あたりの上限）
    pub truncated: bool,
}

/// xlsx の値のあるセル 1 件（行・列は 0 始まり）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct XlsxCell {
    pub row: u32,
    pub col: u32,
    pub value: String,
}

/// pptx のスライド 1 枚
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PptxSlide {
    /// 表示順（1 始まり）
    pub index: u32,
    /// 空でない段落（出現順）
    pub paragraphs: Vec<String>,
}

/// ファイルの内容の表示
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FilePreview {
    /// UTF-8 のテキスト（1 MiB まで）
    Text { content: String, truncated: bool },
    /// Word 文書の段落
    Docx {
        paragraphs: Vec<String>,
        truncated: bool,
    },
    /// Excel ブックのシート（シート数は上限まで）
    Xlsx { sheets: Vec<XlsxSheet> },
    /// PowerPoint のスライド
    Pptx {
        slides: Vec<PptxSlide>,
        truncated: bool,
    },
    /// 内容を表示できない（画面はファイル情報を出す）
    Unavailable { reason: PreviewUnavailableReason },
    /// その版にファイルが無い
    NotFound,
}

/// 指定の時点のファイルの内容を返す（読み取りのみ）
#[tauri::command]
#[specta::specta]
pub async fn preview_file(
    state: tauri::State<'_, AppState>,
    id: String,
    rev: String,
    path: String,
) -> Result<FilePreview, AppError> {
    let store = state.store.clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        preview(&project.path, &rev, &path)
    })
    .await
}

/// 種類ごとの読み取り方
enum Kind {
    /// 抽出する Office 文書
    Office(OfficeKind),
    /// 内容を返さない（画像・pdf）
    Binary,
    /// テキストとして読む（それ以外）
    Text,
}

/// 拡張子で種類を決める。大文字小文字は区別しない
fn kind_of(path: &str) -> Kind {
    if let Some(kind) = office_kind(path) {
        return Kind::Office(kind);
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "pdf" => Kind::Binary,
        _ => Kind::Text,
    }
}

/// 比較の本体（同期処理。呼び出し側が別スレッドで実行する）
fn preview(repo: &Path, point: &str, path: &str) -> Result<FilePreview, AppError> {
    let ops = Ops::new(git_runner());
    let Some(rev) = version::resolve_rev(&ops, repo, point).map_err(AppError::from_ops_error)?
    else {
        return Ok(FilePreview::NotFound);
    };
    match kind_of(path) {
        Kind::Office(kind) => preview_office(&ops, repo, &rev, path, kind),
        Kind::Binary => preview_binary(&ops, repo, &rev, path),
        Kind::Text => preview_text(&ops, repo, &rev, path),
    }
}

/// Office 文書を読んで抽出する。抽出の失敗は `unavailable`
fn preview_office(
    ops: &Ops,
    repo: &Path,
    rev: &str,
    path: &str,
    kind: OfficeKind,
) -> Result<FilePreview, AppError> {
    let limits = Limits::default();
    let bytes = match ops.read_office(repo, rev, path, limits.max_input_bytes) {
        Ok(OfficeFile::Bytes(bytes)) => bytes,
        Ok(OfficeFile::TooLarge) => return Ok(unavailable(PreviewUnavailableReason::TooLarge)),
        Err(e) => return read_error(e),
    };
    let result = match kind {
        OfficeKind::Docx => extract_docx(&bytes, &limits).map(docx_preview),
        OfficeKind::Xlsx => extract_xlsx(&bytes, &limits).and_then(xlsx_preview),
        OfficeKind::Pptx => extract_pptx(&bytes, &limits).map(pptx_preview),
    };
    Ok(match result {
        Ok(preview) => preview,
        Err(err) => unavailable(reason_of(err)),
    })
}

/// 画像・pdf。内容は返さず、存在と読めることだけを確かめて `binary` にする
fn preview_binary(ops: &Ops, repo: &Path, rev: &str, path: &str) -> Result<FilePreview, AppError> {
    match ops.read_text_file(repo, rev, path, TEXT_MAX_BYTES) {
        Ok(_) => Ok(unavailable(PreviewUnavailableReason::Binary)),
        Err(e) => read_error(e),
    }
}

/// テキストとして読む。バイナリ・上限超えは `unavailable`
fn preview_text(ops: &Ops, repo: &Path, rev: &str, path: &str) -> Result<FilePreview, AppError> {
    match ops.read_text_file(repo, rev, path, TEXT_MAX_BYTES) {
        Ok(TextFile::Text(content)) => Ok(FilePreview::Text {
            content,
            // 上限を超えるテキストは一部だけ出さず `too-large` にするため、ここでは切り詰めない
            truncated: false,
        }),
        Ok(TextFile::Binary) => Ok(unavailable(PreviewUnavailableReason::Binary)),
        Ok(TextFile::TooLarge) => Ok(unavailable(PreviewUnavailableReason::TooLarge)),
        Err(e) => read_error(e),
    }
}

/// 読み取りの失敗を写す。存在しなければ `not-found`、パスや形式の不正は `unsupported-format`、
/// そのほかは通常のエラー
fn read_error(err: OpsError) -> Result<FilePreview, AppError> {
    match err {
        OpsError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FilePreview::NotFound),
        OpsError::InvalidInput(_) => Ok(unavailable(PreviewUnavailableReason::UnsupportedFormat)),
        other => Err(AppError::from_ops_error(other)),
    }
}

/// 段落の表示（上限を超えた分は切り捨てて `truncated` にする）
fn docx_preview(mut paragraphs: Vec<String>) -> FilePreview {
    let truncated = paragraphs.len() > MAX_PARAGRAPHS;
    paragraphs.truncate(MAX_PARAGRAPHS);
    FilePreview::Docx {
        paragraphs,
        truncated,
    }
}

/// シートの表示。シート数が上限を超えるブックは全体を `too-large` にする
fn xlsx_preview(sheets: Vec<Sheet>) -> Result<FilePreview, PreviewError> {
    if sheets.len() > MAX_SHEETS {
        return Err(PreviewError::TooLarge);
    }
    Ok(FilePreview::Xlsx {
        sheets: sheets.into_iter().map(sheet_preview).collect(),
    })
}

/// シート 1 枚の表示。行・列の大きさは、切り詰める前の全体で決める
fn sheet_preview(sheet: Sheet) -> XlsxSheet {
    let rows = sheet
        .cells
        .keys()
        .map(|&(row, _)| row.saturating_add(1))
        .max()
        .unwrap_or(0);
    let cols = sheet
        .cells
        .keys()
        .map(|&(_, col)| col.saturating_add(1))
        .max()
        .unwrap_or(0);
    let truncated = sheet.cells.len() > MAX_CELLS_PER_SHEET;
    let cells = sheet
        .cells
        .into_iter()
        .take(MAX_CELLS_PER_SHEET)
        .map(|((row, col), value)| XlsxCell { row, col, value })
        .collect();
    XlsxSheet {
        name: sheet.name,
        rows,
        cols,
        cells,
        truncated,
    }
}

/// スライドの表示（上限を超えた分は切り捨てて `truncated` にする）
fn pptx_preview(mut slides: Vec<Slide>) -> FilePreview {
    let truncated = slides.len() > MAX_SLIDES;
    slides.truncate(MAX_SLIDES);
    FilePreview::Pptx {
        slides: slides
            .into_iter()
            .map(|s| PptxSlide {
                index: to_u32(s.index),
                paragraphs: s.paragraphs,
            })
            .collect(),
        truncated,
    }
}

/// 抽出の失敗を、画面の理由へ写す（内容・パスは含めない）
fn reason_of(err: PreviewError) -> PreviewUnavailableReason {
    match err {
        PreviewError::TooLarge => PreviewUnavailableReason::TooLarge,
        PreviewError::NotZip | PreviewError::Encrypted | PreviewError::Unsupported => {
            PreviewUnavailableReason::UnsupportedFormat
        }
        PreviewError::Corrupt => PreviewUnavailableReason::Corrupt,
    }
}

fn unavailable(reason: PreviewUnavailableReason) -> FilePreview {
    FilePreview::Unavailable { reason }
}

/// 件数を画面の型（u32）へ。上限は十分に大きく、超えることはない
fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
