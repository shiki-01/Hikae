// Office 文書（docx・xlsx）の比較（設計書 3.3 S4、9.3）。2 つの時点の同じパスのファイルを読み、
// 抽出して差分を返す。読み取りのみで、直列キューは通さない（`run_blocking`）。
//
// - 片側にファイルが無い（追加・削除）ときは、その側を空として比べる
// - 抽出できないもの（上限超え・対応しない形式・壊れている）はエラーにせず、`OfficeDiff::Unavailable` で
//   理由を返す。画面はファイル情報の表示に切り替える
// - 画面に返す行数は `ROWS_LIMIT` まで。超えた分は件数（`truncated`）だけ返す
// - 時点は画面の ID（`current` = 作業フォルダ、それ以外は短縮 ID）。`work` も作業フォルダとして受け付ける
// - ファイルの内容・パスは、エラーにも `technical_info` にも出さない

use crate::{git_runner, load_project, run_blocking, AppError, AppState};
use core_ops::{OfficeFile, Ops, OpsError, WORK_REV};
use core_preview::{
    diff_paragraphs, diff_sheets, extract_docx, extract_xlsx, office_kind, Limits, OfficeKind,
    ParagraphDiff, PreviewError, Sheet,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 画面に返す行（段落・セル）の上限。超えた分は件数だけ返す
const ROWS_LIMIT: usize = 1000;

/// 「いま」を表す時点 ID（`diff` コマンドと同じ）
const CURRENT_POINT: &str = "current";

/// 抽出できなかった理由（設計書 5.1。画面は短い文言を出す）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum OfficeUnavailableReason {
    /// 上限（ファイルの大きさ・段落数・セル数など）を超えた
    TooLarge,
    /// 対応していない形式（拡張子と中身が合わない、旧形式、暗号化を含む）
    UnsupportedFormat,
    /// 壊れている
    Corrupt,
}

/// 段落の差分の 1 行
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ParagraphRow {
    /// 変更のない段落（前後の文脈として残したもの）
    Context {
        old_no: u32,
        new_no: u32,
        text: String,
    },
    /// 追加された段落
    Added { new_no: u32, text: String },
    /// 削除された段落
    Removed { old_no: u32, text: String },
    /// 省略された変更のない段落の件数
    Omitted { count: u32 },
}

/// セルの差分 1 件。`before` が無ければ追加、`after` が無ければ削除
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CellRow {
    pub sheet: String,
    /// セル参照（A1 形式）
    pub cell: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Office 文書の比較結果
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum OfficeDiff {
    /// Word 文書の段落差分
    Docx {
        rows: Vec<ParagraphRow>,
        /// `rows` に入らず打ち切った行の数
        truncated: u32,
    },
    /// Excel ブックの差分（シートの追加・削除と、同名シートのセルの差分）
    Xlsx {
        added_sheets: Vec<String>,
        removed_sheets: Vec<String>,
        cells: Vec<CellRow>,
        /// `cells` に入らず打ち切ったセルの数
        truncated: u32,
    },
    /// 抽出できなかった（画面はファイル情報の表示に切り替える）
    Unavailable { reason: OfficeUnavailableReason },
}

/// 2 つの時点の同じパスの Office 文書を比べる。`from`・`to` は画面の時点 ID
#[tauri::command]
#[specta::specta]
pub async fn compare_office(
    state: tauri::State<'_, AppState>,
    id: String,
    from: String,
    to: String,
    path: String,
) -> Result<OfficeDiff, AppError> {
    let store = state.store.clone();
    run_blocking(move || {
        let project = load_project(&store, &id)?;
        compare(&project.path, &from, &to, &path)
    })
    .await
}

/// 読んだ 1 つの時点のファイル
enum Side {
    /// その時点に無い（追加・削除の側）。空として比べる
    Absent,
    /// 読めた内容
    Present(Vec<u8>),
    /// 上限を超えたため読まなかった
    TooLarge,
}

/// 比較の本体（同期処理。呼び出し側が別スレッドで実行する）
fn compare(repo: &Path, from: &str, to: &str, path: &str) -> Result<OfficeDiff, AppError> {
    let Some(kind) = office_kind(path) else {
        return Ok(unavailable(OfficeUnavailableReason::UnsupportedFormat));
    };
    let ops = Ops::new(git_runner());
    let limits = Limits::default();

    let old_rev = resolve_point(&ops, repo, from)?;
    let new_rev = resolve_point(&ops, repo, to)?;
    let old = read_side(&ops, repo, &old_rev, path, limits.max_input_bytes)?;
    let new = read_side(&ops, repo, &new_rev, path, limits.max_input_bytes)?;
    if matches!(old, Side::TooLarge) || matches!(new, Side::TooLarge) {
        return Ok(unavailable(OfficeUnavailableReason::TooLarge));
    }

    let result = match kind {
        OfficeKind::Docx => compare_docx(&old, &new, &limits),
        OfficeKind::Xlsx => compare_xlsx(&old, &new, &limits),
    };
    Ok(match result {
        Ok(diff) => diff,
        Err(err) => unavailable(reason_of(err)),
    })
}

/// 画面の時点 ID を、読むときの rev にする（`current` と `work` は作業フォルダ。それ以外は完全な OID）
fn resolve_point(ops: &Ops, repo: &Path, point: &str) -> Result<String, AppError> {
    if point == CURRENT_POINT || point == WORK_REV {
        return Ok(WORK_REV.to_string());
    }
    ops.full_commit_id(repo, point)
        .map_err(AppError::from_ops_error)
}

/// 1 つの時点のファイルを読む。存在しないものは `Absent`、それ以外の失敗はエラーにする
fn read_side(
    ops: &Ops,
    repo: &Path,
    rev: &str,
    path: &str,
    max_bytes: u64,
) -> Result<Side, AppError> {
    match ops.read_office(repo, rev, path, max_bytes) {
        Ok(OfficeFile::Bytes(bytes)) => Ok(Side::Present(bytes)),
        Ok(OfficeFile::TooLarge) => Ok(Side::TooLarge),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Side::Absent),
        Err(e) => Err(AppError::from_ops_error(e)),
    }
}

/// 段落の差分（docx）。抽出の失敗は `PreviewError` のまま返す
fn compare_docx(old: &Side, new: &Side, limits: &Limits) -> Result<OfficeDiff, PreviewError> {
    let old_paragraphs = paragraphs_of(old, limits)?;
    let new_paragraphs = paragraphs_of(new, limits)?;
    let rows: Vec<ParagraphRow> = diff_paragraphs(&old_paragraphs, &new_paragraphs)
        .into_iter()
        .map(paragraph_row)
        .collect();
    let (rows, truncated) = truncate(rows);
    Ok(OfficeDiff::Docx { rows, truncated })
}

/// シートの差分（xlsx）。抽出の失敗は `PreviewError` のまま返す
fn compare_xlsx(old: &Side, new: &Side, limits: &Limits) -> Result<OfficeDiff, PreviewError> {
    let old_sheets = sheets_of(old, limits)?;
    let new_sheets = sheets_of(new, limits)?;
    let diff = diff_sheets(&old_sheets, &new_sheets);
    let cells: Vec<CellRow> = diff
        .cells
        .into_iter()
        .map(|c| CellRow {
            sheet: c.sheet,
            cell: c.cell,
            before: c.before,
            after: c.after,
        })
        .collect();
    let (cells, truncated) = truncate(cells);
    Ok(OfficeDiff::Xlsx {
        added_sheets: diff.added_sheets,
        removed_sheets: diff.removed_sheets,
        cells,
        truncated,
    })
}

/// 段落の列。その時点に無ければ空
fn paragraphs_of(side: &Side, limits: &Limits) -> Result<Vec<String>, PreviewError> {
    match side {
        Side::Absent | Side::TooLarge => Ok(Vec::new()),
        Side::Present(bytes) => extract_docx(bytes, limits),
    }
}

/// シートの列。その時点に無ければ空
fn sheets_of(side: &Side, limits: &Limits) -> Result<Vec<Sheet>, PreviewError> {
    match side {
        Side::Absent | Side::TooLarge => Ok(Vec::new()),
        Side::Present(bytes) => extract_xlsx(bytes, limits),
    }
}

/// 差分の段落を、画面に渡す形にする
fn paragraph_row(row: ParagraphDiff) -> ParagraphRow {
    match row {
        ParagraphDiff::Context {
            old_no,
            new_no,
            text,
        } => ParagraphRow::Context {
            old_no: to_u32(old_no),
            new_no: to_u32(new_no),
            text,
        },
        ParagraphDiff::Added { new_no, text } => ParagraphRow::Added {
            new_no: to_u32(new_no),
            text,
        },
        ParagraphDiff::Removed { old_no, text } => ParagraphRow::Removed {
            old_no: to_u32(old_no),
            text,
        },
        ParagraphDiff::Omitted { count } => ParagraphRow::Omitted {
            count: to_u32(count),
        },
    }
}

/// 上限を超えた分を切り捨て、切り捨てた件数を返す
fn truncate<T>(mut items: Vec<T>) -> (Vec<T>, u32) {
    let extra = items.len().saturating_sub(ROWS_LIMIT);
    items.truncate(ROWS_LIMIT);
    (items, to_u32(extra))
}

/// 抽出の失敗を、画面の理由へ写す
fn reason_of(err: PreviewError) -> OfficeUnavailableReason {
    match err {
        PreviewError::TooLarge => OfficeUnavailableReason::TooLarge,
        PreviewError::NotZip | PreviewError::Encrypted | PreviewError::Unsupported => {
            OfficeUnavailableReason::UnsupportedFormat
        }
        PreviewError::Corrupt => OfficeUnavailableReason::Corrupt,
    }
}

fn unavailable(reason: OfficeUnavailableReason) -> OfficeDiff {
    OfficeDiff::Unavailable { reason }
}

/// 件数を画面の型（u32）へ。上限は十分に大きく、超えることはない
fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
