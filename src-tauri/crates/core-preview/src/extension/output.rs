// 抽出器の出力（拡張 API の `apiVersion` 1、設計書 11.2）。WASM 抽出器はネイティブの `extract_*` の結果を
// この JSON にして stdout へ書き、ホストが読み戻す。型と JSON の形はここに 1 つだけ置き、両側で使う。
//
// - 成功: `{"output": "paragraphs" | "cells" | "pages", ...}`（形式は `OutputKind` と一致する）
// - 失敗: 終了コードを非 0 にし、stdout に `{"error": "<語彙>"}` を書く。語彙は `error_word` の 4 種類だけ
// - 解析は厳密に行う。未知の `error` 語彙、セルの重複、`rows`・`cols` の食い違い、`truncated` の真は不正とみなす
//   （ホストは不正な出力を `ExtractorFailed` にする。`truncated` は「一部だけ返す」意味で、比較では使わない）

use crate::{PreviewError, Sheet, Slide};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 抽出器が返す出力の形（manifest の `output` と同じ語）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// 段落の列（docx）
    Paragraphs,
    /// シートごとのセル（xlsx）
    Cells,
    /// ページ（pptx のスライド）
    Pages,
}

impl OutputKind {
    /// manifest の `output` の語から決める。知らない語は None
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "paragraphs" => Some(Self::Paragraphs),
            "cells" => Some(Self::Cells),
            "pages" => Some(Self::Pages),
            _ => None,
        }
    }
}

/// 抽出結果（形式は `OutputKind` のどれか）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractOutput {
    /// 段落（docx）
    Paragraphs(Vec<String>),
    /// シートごとのセル（xlsx）。シートはブックの順
    Cells(Vec<Sheet>),
    /// ページ（pptx のスライド）。表示順
    Pages(Vec<Slide>),
}

impl ExtractOutput {
    /// この出力の形式
    pub fn kind(&self) -> OutputKind {
        match self {
            Self::Paragraphs(_) => OutputKind::Paragraphs,
            Self::Cells(_) => OutputKind::Cells,
            Self::Pages(_) => OutputKind::Pages,
        }
    }

    /// stdout に書く JSON にする
    pub fn to_json(self) -> Result<Vec<u8>, serde_json::Error> {
        let wire = match self {
            Self::Paragraphs(paragraphs) => WireOutput::Paragraphs { paragraphs },
            Self::Cells(sheets) => WireOutput::Cells {
                sheets: sheets.into_iter().map(sheet_to_wire).collect(),
            },
            Self::Pages(slides) => WireOutput::Pages {
                pages: slides
                    .into_iter()
                    .map(|s| WirePage {
                        index: s.index,
                        paragraphs: s.paragraphs,
                    })
                    .collect(),
            },
        };
        serde_json::to_vec(&wire)
    }
}

/// 失敗の JSON（`{"error": "<語彙>"}`）。内容・パスは含めない
pub fn error_json(err: PreviewError) -> Vec<u8> {
    let wire = WireError {
        error: error_word(err).to_owned(),
    };
    // 固定の形のため、直列化は失敗しない。失敗しても空の出力になり、ホストは不正な出力として扱う
    serde_json::to_vec(&wire).unwrap_or_default()
}

/// 失敗の語彙（設計書 11.2 の固定の語。画面の文言の種類と対応する）
pub fn error_word(err: PreviewError) -> &'static str {
    match err {
        PreviewError::TooLarge => "too-large",
        // zip ではないことと対応しない内容は、画面では同じ扱い（対応しない形式）
        PreviewError::NotZip | PreviewError::Unsupported => "unsupported-format",
        PreviewError::Encrypted => "encrypted",
        PreviewError::Corrupt => "corrupt",
    }
}

/// 失敗の語彙を、抽出の失敗へ戻す。知らない語は None
pub fn error_from_word(word: &str) -> Option<PreviewError> {
    match word {
        "too-large" => Some(PreviewError::TooLarge),
        "unsupported-format" => Some(PreviewError::Unsupported),
        "encrypted" => Some(PreviewError::Encrypted),
        "corrupt" => Some(PreviewError::Corrupt),
        _ => None,
    }
}

/// 抽出器の成功出力を読み戻せなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// 形式が規約と違う（JSON の構文、必須の項目、形式の食い違い、重複など）
    Malformed,
    /// `truncated` が真（一部だけの結果。全体を比べられないため受け取らない）
    Truncated,
}

/// 成功時の stdout を読み戻す。`expected` と形式が違えば `Malformed`
pub fn decode_output(bytes: &[u8], expected: OutputKind) -> Result<ExtractOutput, DecodeError> {
    let wire: WireOutput = serde_json::from_slice(bytes).map_err(|_| DecodeError::Malformed)?;
    let output = match wire {
        WireOutput::Paragraphs { paragraphs } => ExtractOutput::Paragraphs(paragraphs),
        WireOutput::Cells { sheets } => ExtractOutput::Cells(
            sheets
                .into_iter()
                .map(sheet_from_wire)
                .collect::<Result<_, _>>()?,
        ),
        WireOutput::Pages { pages } => ExtractOutput::Pages(
            pages
                .into_iter()
                .map(|p| Slide {
                    index: p.index,
                    paragraphs: p.paragraphs,
                })
                .collect(),
        ),
    };
    if output.kind() != expected {
        return Err(DecodeError::Malformed);
    }
    Ok(output)
}

/// 失敗の JSON を読む。`error` の語が知らないもの、形が違うものは None
pub fn decode_error(bytes: &[u8]) -> Option<PreviewError> {
    let wire: WireError = serde_json::from_slice(bytes).ok()?;
    error_from_word(&wire.error)
}

// ---- 以下は JSON の形（serde の型）。公開しない ----

#[derive(Serialize, Deserialize)]
#[serde(tag = "output", rename_all = "lowercase")]
enum WireOutput {
    Paragraphs { paragraphs: Vec<String> },
    Cells { sheets: Vec<WireSheet> },
    Pages { pages: Vec<WirePage> },
}

#[derive(Serialize, Deserialize)]
struct WireSheet {
    name: String,
    /// 値のあるセルの最大行 + 1（値が無ければ 0）
    rows: u32,
    /// 値のあるセルの最大列 + 1（値が無ければ 0）
    cols: u32,
    cells: Vec<WireCell>,
    /// 一部だけを返したか。現在は常に偽（上限を超えたら抽出器がエラーを返す）
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct WireCell {
    row: u32,
    col: u32,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct WirePage {
    index: usize,
    paragraphs: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct WireError {
    error: String,
}

/// シートの行数・列数（値のあるセルの最大の位置 + 1）
fn sheet_extent(cells: &BTreeMap<(u32, u32), String>) -> (u32, u32) {
    let rows = cells
        .keys()
        .map(|&(row, _)| row.saturating_add(1))
        .max()
        .unwrap_or(0);
    let cols = cells
        .keys()
        .map(|&(_, col)| col.saturating_add(1))
        .max()
        .unwrap_or(0);
    (rows, cols)
}

fn sheet_to_wire(sheet: Sheet) -> WireSheet {
    let (rows, cols) = sheet_extent(&sheet.cells);
    WireSheet {
        name: sheet.name,
        rows,
        cols,
        cells: sheet
            .cells
            .into_iter()
            .map(|((row, col), value)| WireCell { row, col, value })
            .collect(),
        truncated: false,
    }
}

fn sheet_from_wire(sheet: WireSheet) -> Result<Sheet, DecodeError> {
    if sheet.truncated {
        return Err(DecodeError::Truncated);
    }
    let mut cells = BTreeMap::new();
    for cell in sheet.cells {
        if cells.insert((cell.row, cell.col), cell.value).is_some() {
            return Err(DecodeError::Malformed);
        }
    }
    if sheet_extent(&cells) != (sheet.rows, sheet.cols) {
        return Err(DecodeError::Malformed);
    }
    Ok(Sheet {
        name: sheet.name,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(name: &str, cells: &[((u32, u32), &str)]) -> Sheet {
        Sheet {
            name: name.to_owned(),
            cells: cells.iter().map(|&(k, v)| (k, v.to_owned())).collect(),
        }
    }

    #[test]
    fn cells_round_trip_through_json() -> Result<(), DecodeError> {
        let original = ExtractOutput::Cells(vec![sheet("S", &[((0, 0), "a"), ((2, 1), "b")])]);
        let json = original
            .clone()
            .to_json()
            .map_err(|_| DecodeError::Malformed)?;
        assert_eq!(decode_output(&json, OutputKind::Cells)?, original);
        Ok(())
    }

    #[test]
    fn a_mismatched_kind_is_malformed() {
        let json = ExtractOutput::Paragraphs(vec!["x".into()])
            .to_json()
            .unwrap_or_default();
        assert_eq!(
            decode_output(&json, OutputKind::Pages),
            Err(DecodeError::Malformed)
        );
    }

    #[test]
    fn duplicate_cells_and_wrong_extent_are_malformed() {
        let dup = br#"{"output":"cells","sheets":[{"name":"S","rows":1,"cols":1,"truncated":false,"cells":[{"row":0,"col":0,"value":"a"},{"row":0,"col":0,"value":"b"}]}]}"#;
        assert_eq!(
            decode_output(dup, OutputKind::Cells),
            Err(DecodeError::Malformed)
        );
        let extent = br#"{"output":"cells","sheets":[{"name":"S","rows":5,"cols":1,"truncated":false,"cells":[{"row":0,"col":0,"value":"a"}]}]}"#;
        assert_eq!(
            decode_output(extent, OutputKind::Cells),
            Err(DecodeError::Malformed)
        );
    }

    #[test]
    fn truncated_sheets_are_not_accepted() {
        let json = br#"{"output":"cells","sheets":[{"name":"S","rows":0,"cols":0,"truncated":true,"cells":[]}]}"#;
        assert_eq!(
            decode_output(json, OutputKind::Cells),
            Err(DecodeError::Truncated)
        );
    }

    #[test]
    fn error_words_map_both_ways_for_the_wire_vocabulary() {
        for err in [
            PreviewError::TooLarge,
            PreviewError::Unsupported,
            PreviewError::Encrypted,
            PreviewError::Corrupt,
        ] {
            assert_eq!(error_from_word(error_word(err)), Some(err));
        }
        // zip ではない入力は、対応しない形式として扱う（語彙は 4 種類）
        assert_eq!(error_word(PreviewError::NotZip), "unsupported-format");
        assert_eq!(error_from_word("panic"), None);
        assert_eq!(
            decode_error(&error_json(PreviewError::Encrypted)),
            Some(PreviewError::Encrypted)
        );
        assert_eq!(decode_error(b"{\"error\":\"nope\"}"), None);
    }
}
