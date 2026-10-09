// xlsx のセルの値の抽出（設計書 3.3 S4）。シートごとに、(行, 列) から値の文字列への対応を作る。
//
// - 値は保存されている結果の値（数式ではない）。calamine の `worksheet_range` が返す値を文字列にする
// - 空セル（値の無いセル、空文字の文字列）は含めない
// - 数値は整数なら小数点を付けない（120.0 → "120"）。それ以外は Rust の既定の表示（最短の十進表記）
// - 真偽値は "TRUE" / "FALSE"、エラー値は Excel の表記（"#DIV/0!" など）
// - 日付・時刻は、calamine が保持する Excel のシリアル値（小数）を数値と同じ表記にする。
//   日付の文字列への変換（表示形式・1904 年系の扱い）はしない
//
// 安全上の扱い:
// - すべてのエントリを 1 つずつ読み、展開後の合計を上限で止める。calamine が読むものは、このうちの部分集合
//   なので、calamine の展開量も同じ上限に収まる
// - calamine はシートのセル参照の矩形の全体を確保する。そのため、calamine に渡す前に、すべての XML の
//   SpreadsheetML の `c` 要素を走査し、セル数と矩形の面積を上限で確かめる（`Limits::max_sheet_area`）

use crate::archive::Archive;
use crate::{Limits, PreviewError};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use quick_xml::events::Event;
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::NsReader;
use std::collections::BTreeMap;
use std::io::Cursor;

/// SpreadsheetML の名前空間（セル要素 `c` の判定）
const SPREADSHEET_NS: &[u8] = b"http://schemas.openxmlformats.org/spreadsheetml/2006/main";
/// ブックの部品の名前（xlsx かどうかの判定に使う）
const WORKBOOK_PART: &str = "xl/workbook.xml";

/// シート 1 枚。値のあるセルだけを持つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    /// シート名（ブックの順）
    pub name: String,
    /// (行, 列) → 値の文字列。行と列は 0 始まり（A1 は (0, 0)）。順序は行 → 列
    pub cells: BTreeMap<(u32, u32), String>,
}

/// xlsx の全シートを、ブックの順に返す
pub fn extract_xlsx(bytes: &[u8], limits: &Limits) -> Result<Vec<Sheet>, PreviewError> {
    let mut archive = Archive::open(bytes, limits)?;
    if archive.index_of(WORKBOOK_PART).is_none() {
        return Err(PreviewError::Unsupported);
    }
    // 展開後の合計を確かめ、セル参照の矩形を calamine に渡す前に検査する。
    // 部品の種類は名前ではなく中身で決める（名前の大文字小文字や拡張子で抜け道を作らないため）。
    // XML として読まれうるのは、先頭が `<` の部品だけ
    for index in 0..archive.len() {
        let data = archive.read_index(index)?;
        if looks_like_xml(&data) {
            scan_cell_bounds(&data, limits)?;
        }
    }

    let mut workbook: Xlsx<Cursor<&[u8]>> =
        open_workbook_from_rs(Cursor::new(bytes)).map_err(|_| PreviewError::Corrupt)?;
    let mut total_cells = 0usize;
    let mut sheets = Vec::new();
    for name in workbook.sheet_names() {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|_| PreviewError::Corrupt)?;
        let mut cells = BTreeMap::new();
        if let Some((top, left)) = range.start() {
            for (row, col, value) in range.used_cells() {
                let text = value_text(value);
                if text.is_empty() {
                    continue;
                }
                total_cells += 1;
                if total_cells > limits.max_cells {
                    return Err(PreviewError::TooLarge);
                }
                let row = top.saturating_add(u32::try_from(row).unwrap_or(u32::MAX));
                let col = left.saturating_add(u32::try_from(col).unwrap_or(u32::MAX));
                cells.insert((row, col), text);
            }
        }
        sheets.push(Sheet { name, cells });
    }
    Ok(sheets)
}

/// セルの値を文字列にする（規則はモジュールの先頭に書いたもの）
fn value_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => float_text(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(dt) => float_text(dt.as_f64()),
        Data::Error(e) => e.to_string(),
    }
}

/// 数値の文字列。整数（大きさが十分に小さいもの）は小数点を付けない
fn float_text(value: f64) -> String {
    // 2^53 未満の整数は f64 で正確に表せるため、整数として書いて差し支えない
    const EXACT_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0;
    if value.is_finite() && value.fract() == 0.0 && value.abs() < EXACT_INTEGER_LIMIT {
        // 負のゼロも "0" にする
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

/// 中身が XML の部品か（先頭の空白と UTF-8 の BOM を飛ばして、`<` で始まるか）
fn looks_like_xml(data: &[u8]) -> bool {
    let body = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    body.iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|&b| b == b'<')
}

/// シートの矩形の下限・上限（行・列、0 始まり）
#[derive(Clone, Copy)]
struct Bounds {
    min_row: u32,
    max_row: u32,
    min_col: u32,
    max_col: u32,
}

impl Bounds {
    fn include(&mut self, (row, col): (u32, u32)) {
        self.min_row = self.min_row.min(row);
        self.max_row = self.max_row.max(row);
        self.min_col = self.min_col.min(col);
        self.max_col = self.max_col.max(col);
    }

    /// 矩形の面積（オーバーフローは飽和させる）
    fn area(&self) -> u64 {
        let rows = u64::from(self.max_row - self.min_row) + 1;
        let cols = u64::from(self.max_col - self.min_col) + 1;
        rows.saturating_mul(cols)
    }
}

/// XML の中の SpreadsheetML のセル要素 `c` を数え、参照の矩形が上限を超えないか確かめる。
/// 参照が読めないセルは `Corrupt`（calamine も位置を決められない）
fn scan_cell_bounds(xml: &[u8], limits: &Limits) -> Result<(), PreviewError> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut bounds: Option<Bounds> = None;
    let mut count: u64 = 0;
    loop {
        buf.clear();
        let (ns, event) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(|_| PreviewError::Corrupt)?;
        let element = match event {
            Event::Start(e) | Event::Empty(e) => e,
            Event::Eof => break,
            _ => continue,
        };
        let is_cell = matches!(ns, ResolveResult::Bound(Namespace(uri)) if uri == SPREADSHEET_NS)
            && element.local_name().as_ref() == b"c";
        if !is_cell {
            continue;
        }
        // 同じ要素を何度も書く XML で、メモリを使い切らないように数で止める
        count += 1;
        if count > limits.max_sheet_area {
            return Err(PreviewError::TooLarge);
        }
        let reference = element
            .try_get_attribute("r")
            .map_err(|_| PreviewError::Corrupt)?
            .ok_or(PreviewError::Corrupt)?;
        // セル参照は ASCII の英数字だけなので、エスケープを解かなくても形は確かめられる。
        // エスケープを含むものは参照として読めないため、そのまま Corrupt にする
        let value = std::str::from_utf8(&reference.value).map_err(|_| PreviewError::Corrupt)?;
        let position = parse_cell_ref(value).ok_or(PreviewError::Corrupt)?;
        match bounds.as_mut() {
            Some(b) => b.include(position),
            None => {
                bounds = Some(Bounds {
                    min_row: position.0,
                    max_row: position.0,
                    min_col: position.1,
                    max_col: position.1,
                })
            }
        }
    }
    if let Some(b) = bounds {
        if b.area() > limits.max_sheet_area {
            return Err(PreviewError::TooLarge);
        }
    }
    Ok(())
}

/// A1 形式のセル参照を (行, 列)（0 始まり）にする。形が違えば None
fn parse_cell_ref(text: &str) -> Option<(u32, u32)> {
    let bytes = text.as_bytes();
    let split = bytes.iter().position(u8::is_ascii_digit)?;
    let (letters, digits) = bytes.split_at(split);
    // 列は最大 3 文字（XFD）。それ以上は範囲外
    if letters.is_empty() || letters.len() > 3 || !letters.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut col: u32 = 0;
    for b in letters {
        col = col * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1);
    }
    let row: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    if row == 0 || col == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{num, row, text, xlsx_of, zip_of};

    fn limits() -> Limits {
        Limits::default()
    }

    /// セルの値を (行, 列) の列にする（テストの読みやすさのため）
    fn values(sheet: &Sheet) -> Vec<((u32, u32), String)> {
        sheet.cells.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    #[test]
    fn values_are_read_as_text_and_integers_have_no_decimal_point() -> Result<(), PreviewError> {
        let data = row(
            1,
            &(text("A1", "name") + &num("B1", "120") + &num("C1", "0.5")),
        );
        let sheets = extract_xlsx(&xlsx_of(&[("Sheet1", &data)]), &limits())?;
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].name, "Sheet1");
        assert_eq!(
            values(&sheets[0]),
            vec![
                ((0, 0), "name".to_owned()),
                ((0, 1), "120".to_owned()),
                ((0, 2), "0.5".to_owned()),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_formula_contributes_its_stored_result_not_the_formula() -> Result<(), PreviewError> {
        let data = row(
            1,
            r#"<c r="A1"><f>1+1</f><v>2</v></c><c r="B1" t="str"><f>"x"&amp;"y"</f><v>xy</v></c>"#,
        );
        let sheets = extract_xlsx(&xlsx_of(&[("S", &data)]), &limits())?;
        assert_eq!(
            values(&sheets[0]),
            vec![((0, 0), "2".to_owned()), ((0, 1), "xy".to_owned())]
        );
        Ok(())
    }

    #[test]
    fn empty_cells_and_empty_strings_are_not_included() -> Result<(), PreviewError> {
        let data = row(
            1,
            &(r#"<c r="A1" s="1"/>"#.to_owned() + &text("B1", "") + &num("C1", "1")),
        );
        let sheets = extract_xlsx(&xlsx_of(&[("S", &data)]), &limits())?;
        assert_eq!(values(&sheets[0]), vec![((0, 2), "1".to_owned())]);
        Ok(())
    }

    #[test]
    fn booleans_and_error_values_are_written_as_excel_shows_them() -> Result<(), PreviewError> {
        let data = row(
            1,
            r#"<c r="A1" t="b"><v>1</v></c><c r="B1" t="b"><v>0</v></c><c r="C1" t="e"><v>#DIV/0!</v></c>"#,
        );
        let sheets = extract_xlsx(&xlsx_of(&[("S", &data)]), &limits())?;
        assert_eq!(
            values(&sheets[0]),
            vec![
                ((0, 0), "TRUE".to_owned()),
                ((0, 1), "FALSE".to_owned()),
                ((0, 2), "#DIV/0!".to_owned()),
            ]
        );
        Ok(())
    }

    #[test]
    fn sheets_keep_the_workbook_order() -> Result<(), PreviewError> {
        let first = row(1, &num("A1", "1"));
        let second = row(1, &num("A1", "2"));
        let sheets = extract_xlsx(
            &xlsx_of(&[("Second", &second), ("First", &first)]),
            &limits(),
        )?;
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Second", "First"]);
        Ok(())
    }

    #[test]
    fn a_zip_without_a_workbook_is_unsupported() {
        let zip = zip_of(&[("word/document.xml", b"<x/>")], false);
        assert_eq!(
            extract_xlsx(&zip, &limits()),
            Err(PreviewError::Unsupported)
        );
    }

    #[test]
    fn a_compound_file_is_not_a_zip() {
        // 旧形式の .xls（OLE 複合ファイル）の先頭
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        bytes.extend_from_slice(&[0u8; 64]);
        assert_eq!(extract_xlsx(&bytes, &limits()), Err(PreviewError::NotZip));
    }

    #[test]
    fn a_broken_workbook_is_corrupt() {
        let zip = zip_of(&[("xl/workbook.xml", b"<workbook><sheets>")], false);
        assert_eq!(extract_xlsx(&zip, &limits()), Err(PreviewError::Corrupt));
    }

    #[test]
    fn a_cell_reference_that_is_not_a_reference_is_corrupt() {
        // XML としては正しいが、参照として読めない
        let data = row(1, r#"<c r="??"/>"#);
        let bytes = xlsx_of(&[("S", &data)]);
        assert_eq!(extract_xlsx(&bytes, &limits()), Err(PreviewError::Corrupt));
    }

    #[test]
    fn too_many_cells_is_too_large() {
        let data = row(1, &(num("A1", "1") + &num("B1", "2") + &num("C1", "3")));
        let small = Limits {
            max_cells: 2,
            ..Limits::default()
        };
        assert_eq!(
            extract_xlsx(&xlsx_of(&[("S", &data)]), &small),
            Err(PreviewError::TooLarge)
        );
    }

    #[test]
    fn a_far_away_cell_is_refused_before_calamine_allocates_the_range() {
        // A1 と XFD1048576 だけでも、矩形は約 1.7 * 10^10 セルになる。calamine に渡す前に止める
        let data = row(1, &num("A1", "1")) + &row(1048576, &num("XFD1048576", "2"));
        assert_eq!(
            extract_xlsx(&xlsx_of(&[("S", &data)]), &limits()),
            Err(PreviewError::TooLarge)
        );
    }

    #[test]
    fn a_sheet_part_is_checked_whatever_its_name_is() {
        // 部品の名前を大文字にする・拡張子を外す、のどちらでも、中身で走査される
        let sheet = format!(
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{}{}</sheetData></worksheet>"#,
            row(1, &num("A1", "1")),
            row(1048576, &num("XFD1048576", "2"))
        );
        for name in ["xl/worksheets/SHEET1.XML", "xl/worksheets/sheet1"] {
            let workbook = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
            let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/SHEET1.XML"/></Relationships>"#;
            let package = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
            let zip = zip_of(
                &[
                    ("_rels/.rels", package.as_bytes()),
                    ("xl/workbook.xml", workbook.as_bytes()),
                    ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
                    (name, sheet.as_bytes()),
                ],
                true,
            );
            assert_eq!(
                extract_xlsx(&zip, &limits()),
                Err(PreviewError::TooLarge),
                "{name}"
            );
        }
    }

    #[test]
    fn a_sheet_with_many_cell_elements_is_refused_by_count() {
        let mut cells = String::new();
        for _ in 0..50 {
            cells.push_str(r#"<c r="A1"/>"#);
        }
        let data = row(1, &cells);
        let small = Limits {
            max_sheet_area: 10,
            ..Limits::default()
        };
        assert_eq!(
            extract_xlsx(&xlsx_of(&[("S", &data)]), &small),
            Err(PreviewError::TooLarge)
        );
    }

    #[test]
    fn the_expanded_total_is_limited() {
        let data = row(1, &text("A1", &"x".repeat(5000)));
        let small = Limits {
            max_uncompressed_bytes: 1000,
            ..Limits::default()
        };
        assert_eq!(
            extract_xlsx(&xlsx_of(&[("S", &data)]), &small),
            Err(PreviewError::TooLarge)
        );
    }

    #[test]
    fn a_cell_reference_parses_to_zero_based_row_and_column() {
        assert_eq!(parse_cell_ref("A1"), Some((0, 0)));
        assert_eq!(parse_cell_ref("b3"), Some((2, 1)));
        assert_eq!(parse_cell_ref("AA10"), Some((9, 26)));
        assert_eq!(parse_cell_ref("XFD1048576"), Some((1_048_575, 16_383)));
        assert_eq!(parse_cell_ref("A0"), None);
        assert_eq!(parse_cell_ref("1A"), None);
        assert_eq!(parse_cell_ref("ABCD1"), None);
        assert_eq!(parse_cell_ref(""), None);
    }

    #[test]
    fn xml_parts_are_recognised_by_content() {
        assert!(looks_like_xml(b"<worksheet/>"));
        assert!(looks_like_xml(b"  \r\n<worksheet/>"));
        assert!(looks_like_xml(b"\xEF\xBB\xBF<worksheet/>"));
        assert!(!looks_like_xml(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_xml(b"PK\x03\x04"));
        assert!(!looks_like_xml(b""));
    }

    #[test]
    fn integers_and_decimals_are_written_plainly() {
        assert_eq!(float_text(120.0), "120");
        assert_eq!(float_text(-3.0), "-3");
        assert_eq!(float_text(0.5), "0.5");
        assert_eq!(float_text(-0.0), "0");
        assert_eq!(float_text(1e20), "100000000000000000000");
    }
}
