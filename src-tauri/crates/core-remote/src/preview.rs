// モバイル版のファイル表示用の変換（設計書 14.4）。
//
// 端末の画面で表示できる形へ、ファイルの内容を Rust 側で振り分ける。
// - 判定は拡張子で行う（大文字小文字を区別しない）。中身の検証はしない（画像・PDF として返し、表示側が失敗を扱う）
// - テキストは UTF-8 として正しく、先頭 8 KiB に NUL が無いものだけを返す。それ以外は表示しない
// - docx・xlsx・pptx は `core-preview` のネイティブ抽出で文字だけを取り出す（WASM 抽出器と拡張ホストは使わない）
// - 入力のバイト列は読むだけで、ファイルへは書かない。任意の入力で panic せず、表示できないものは `Unsupported` にする

use core_preview::{
    extract_docx, extract_pptx, extract_xlsx, office_kind, Limits, OfficeKind, Sheet,
};
use std::collections::BTreeMap;
use std::path::Path;

/// テキストとして返す本文の上限（バイト）。超える分は UTF-8 の文字境界で切る
const TEXT_MAX_BYTES: usize = 1024 * 1024;
/// NUL の有無を調べる先頭の大きさ（バイト）
const NUL_SCAN_BYTES: usize = 8 * 1024;
/// xlsx の表示の上限（行・列）。超える分は切り捨てる
const SHEET_MAX_ROWS: usize = 500;
const SHEET_MAX_COLS: usize = 50;
/// UTF-8 の BOM（テキストの先頭にあれば取り除く）
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// 種類が分からないときの MIME
const FALLBACK_MIME: &str = "application/octet-stream";

/// 抽出できる Office 文書の種類（表示の見出しと列の扱いを決める）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    /// Word（段落だけを並べる）
    Docx,
    /// Excel（シートごとに、セルの表）
    Xlsx,
    /// PowerPoint（スライドごとに、段落を並べる）
    Pptx,
}

/// 文書の 1 区画。docx は 1 つ、pptx はスライドごと、xlsx はシートごとに作る
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSection {
    /// 見出し（docx は無し、pptx は「スライド N」、xlsx はシート名）
    pub title: Option<String>,
    /// 段落（空でないテキストの並び）
    pub paragraphs: Vec<String>,
    /// 表（xlsx のみ。A1 を起点とする最大行 × 最大列の長方形。空セルは空文字）
    pub rows: Option<Vec<Vec<String>>>,
}

/// 表示する内容。種類ごとに画面の描き方が決まる
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// テキスト（UTF-8）。`truncated` は上限で切ったことを示す
    Text { text: String, truncated: bool },
    /// 画像（`<img>` で表示する。SVG もスクリプトは実行されない）
    Image { mime: String, bytes: Vec<u8> },
    /// PDF（表示側で描く）
    Pdf { bytes: Vec<u8> },
    /// docx・xlsx・pptx の文字
    Document {
        format: DocFormat,
        sections: Vec<DocSection>,
    },
    /// 表示できない（対応外の形式、抽出の失敗、テキストとして読めない内容）
    Unsupported,
}

/// ファイル名と内容から、表示する内容を決める。
///
/// 拡張子が画像・PDF・Office のものはその種類として返す（中身とは照合しない）。
/// それ以外は、テキストとして読めるものだけを `Text` にし、読めないものは `Unsupported` にする。
pub fn build_preview(file_name: &str, bytes: Vec<u8>) -> Preview {
    let ext = extension_of(file_name);
    if let Some(mime) = ext.as_deref().and_then(image_mime) {
        return Preview::Image {
            mime: mime.to_string(),
            bytes,
        };
    }
    if ext.as_deref() == Some("pdf") {
        return Preview::Pdf { bytes };
    }
    if let Some(kind) = office_kind(file_name) {
        return office_preview(kind, &bytes);
    }
    text_preview(&bytes)
}

/// ファイル名の拡張子から MIME を決める（小文字化して比べる）。不明なものは `application/octet-stream`
pub fn mime_for(file_name: &str) -> &'static str {
    let ext = extension_of(file_name);
    let ext = ext.as_deref();
    if let Some(mime) = ext.and_then(image_mime) {
        return mime;
    }
    match ext {
        Some("pdf") => "application/pdf",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => FALLBACK_MIME,
    }
}

/// 拡張子（小文字化）。`Path` の規則に従う（`.png` のような隠しファイルは拡張子なし）
fn extension_of(file_name: &str) -> Option<String> {
    Path::new(file_name)
        .extension()?
        .to_str()
        .map(str::to_ascii_lowercase)
}

/// 画像の拡張子から MIME を決める
fn image_mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// docx・xlsx・pptx の文字を取り出す。抽出に失敗したら（暗号化・破損・上限超えを含む）`Unsupported`
fn office_preview(kind: OfficeKind, bytes: &[u8]) -> Preview {
    let limits = Limits::default();
    match kind {
        OfficeKind::Docx => match extract_docx(bytes, &limits) {
            Ok(paragraphs) => Preview::Document {
                format: DocFormat::Docx,
                sections: vec![DocSection {
                    title: None,
                    paragraphs,
                    rows: None,
                }],
            },
            Err(_) => Preview::Unsupported,
        },
        OfficeKind::Xlsx => match extract_xlsx(bytes, &limits) {
            Ok(sheets) => Preview::Document {
                format: DocFormat::Xlsx,
                sections: sheets.into_iter().map(sheet_section).collect(),
            },
            Err(_) => Preview::Unsupported,
        },
        OfficeKind::Pptx => match extract_pptx(bytes, &limits) {
            Ok(slides) => Preview::Document {
                format: DocFormat::Pptx,
                sections: slides
                    .into_iter()
                    .map(|slide| DocSection {
                        title: Some(format!("スライド {}", slide.index)),
                        paragraphs: slide.paragraphs,
                        rows: None,
                    })
                    .collect(),
            },
            Err(_) => Preview::Unsupported,
        },
    }
}

/// シート 1 枚を、シート名の見出しと表の区画にする
fn sheet_section(sheet: Sheet) -> DocSection {
    DocSection {
        title: Some(sheet.name),
        paragraphs: Vec::new(),
        rows: Some(grid(&sheet.cells)),
    }
}

/// セルの値（(行, 列)、0 始まり）を、A1 を起点とする最大行 × 最大列の二次元配列にする。
/// 表示の上限（500 行 × 50 列）を超える分は捨てる。セルが 1 つも無ければ空の配列
fn grid(cells: &BTreeMap<(u32, u32), String>) -> Vec<Vec<String>> {
    let total_rows = cells
        .keys()
        .map(|&(row, _)| index_len(row))
        .max()
        .unwrap_or(0);
    let total_cols = cells
        .keys()
        .map(|&(_, col)| index_len(col))
        .max()
        .unwrap_or(0);
    if total_rows == 0 || total_cols == 0 {
        return Vec::new();
    }
    let rows = total_rows.min(SHEET_MAX_ROWS);
    let cols = total_cols.min(SHEET_MAX_COLS);
    let mut table = vec![vec![String::new(); cols]; rows];
    for (&(row, col), text) in cells {
        // 上限の外のセルは、表の範囲に入らないため捨てられる
        if let Some(cell) = table
            .get_mut(index_of(row))
            .and_then(|line| line.get_mut(index_of(col)))
        {
            *cell = text.clone();
        }
    }
    table
}

/// 0 始まりの位置から、そこまでを含む長さ（位置 + 1）。飽和させて溢れを防ぐ
fn index_len(index: u32) -> usize {
    index_of(index).saturating_add(1)
}

/// 0 始まりの位置を `usize` にする（32 bit 以上の環境では常に収まる）
fn index_of(index: u32) -> usize {
    usize::try_from(index).unwrap_or(usize::MAX)
}

/// テキストとして表示する。UTF-8 でなく、先頭 8 KiB に NUL があるもの（バイナリ）は `Unsupported`。
/// 空のファイルは空のテキストになる
fn text_preview(bytes: &[u8]) -> Preview {
    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let head = &body[..body.len().min(NUL_SCAN_BYTES)];
    if head.contains(&0) {
        return Preview::Unsupported;
    }
    // 全体を検査する（先頭だけ正しくて後ろが壊れている内容も、テキストとしては表示しない）
    let Ok(text) = std::str::from_utf8(body) else {
        return Preview::Unsupported;
    };
    let truncated = text.len() > TEXT_MAX_BYTES;
    let shown = if truncated {
        &text[..char_floor(text, TEXT_MAX_BYTES)]
    } else {
        text
    };
    Preview::Text {
        text: shown.to_string(),
        truncated,
    }
}

/// `max` バイト以下で最も大きい文字境界。マルチバイト文字の途中で切らないため
fn char_floor(text: &str, max: usize) -> usize {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    /// 名前と中身（文字列）の組から zip を作る（メモリ上だけ。圧縮あり）
    fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, data) in parts {
            writer.start_file(*name, options).expect("start_file");
            writer.write_all(data.as_bytes()).expect("write_all");
        }
        writer.finish().expect("finish").into_inner()
    }

    const XML_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

    /// 段落の並びから最小の docx（`word/document.xml` だけ）を作る
    fn docx_of(paragraphs: &[&str]) -> Vec<u8> {
        let body: String = paragraphs
            .iter()
            .map(|text| format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#))
            .collect();
        let document = format!(
            r#"{XML_HEAD}
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        zip_of(&[
            ("[Content_Types].xml", "<Types/>"),
            ("word/document.xml", &document),
        ])
    }

    /// 0 始まりの (行, 列) から A1 形式の参照にする
    fn cell_ref(row: u32, col: u32) -> String {
        let mut n = col + 1;
        let mut letters = String::new();
        while n > 0 {
            let rem = (n - 1) % 26;
            letters.insert(0, char::from(b'A' + rem as u8));
            n = (n - 1) / 26;
        }
        format!("{letters}{}", row + 1)
    }

    /// (行, 列, 値) の並びから `<sheetData>` の中身を作る。数値として読めるものは数値のセルにする
    fn sheet_data(cells: &[(u32, u32, &str)]) -> String {
        let mut sorted = cells.to_vec();
        sorted.sort();
        let mut rows: BTreeMap<u32, String> = BTreeMap::new();
        for &(row, col, value) in &sorted {
            let reference = cell_ref(row, col);
            let xml = if value.parse::<f64>().is_ok() {
                format!(r#"<c r="{reference}"><v>{value}</v></c>"#)
            } else {
                format!(r#"<c r="{reference}" t="inlineStr"><is><t>{value}</t></is></c>"#)
            };
            rows.entry(row).or_default().push_str(&xml);
        }
        rows.iter()
            .map(|(row, cells)| format!(r#"<row r="{}">{cells}</row>"#, row + 1))
            .collect()
    }

    /// (シート名, `<sheetData>` の中身) の並びから最小の xlsx を作る
    fn xlsx_of(sheets: &[(&str, String)]) -> Vec<u8> {
        let mut workbook_sheets = String::new();
        let mut rels = String::new();
        let mut parts: Vec<(String, String)> = Vec::new();
        for (i, (name, data)) in sheets.iter().enumerate() {
            let n = i + 1;
            workbook_sheets.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            ));
            parts.push((
                format!("xl/worksheets/sheet{n}.xml"),
                format!(
                    r#"{XML_HEAD}
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
                ),
            ));
        }
        let mut all: Vec<(String, String)> = vec![
            ("[Content_Types].xml".to_string(), "<Types/>".to_string()),
            (
                "_rels/.rels".to_string(),
                format!(
                    r#"{XML_HEAD}
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
                ),
            ),
            (
                "xl/workbook.xml".to_string(),
                format!(
                    r#"{XML_HEAD}
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{workbook_sheets}</sheets></workbook>"#
                ),
            ),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                format!(
                    r#"{XML_HEAD}
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
                ),
            ),
        ];
        all.extend(parts);
        let refs: Vec<(&str, &str)> = all
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_str()))
            .collect();
        zip_of(&refs)
    }

    /// スライドごとの段落の並びから最小の pptx を作る（表示順は presentation.xml の並び）
    fn pptx_of(slides: &[&[&str]]) -> Vec<u8> {
        let mut sld_ids = String::new();
        let mut rels = String::new();
        let mut parts: Vec<(String, String)> = Vec::new();
        for (i, paragraphs) in slides.iter().enumerate() {
            let n = i + 1;
            sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{n}"/>"#, 255 + n));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{n}.xml"/>"#
            ));
            let body: String = paragraphs
                .iter()
                .map(|text| format!(r#"<a:p><a:r><a:t>{text}</a:t></a:r></a:p>"#))
                .collect();
            parts.push((
                format!("ppt/slides/slide{n}.xml"),
                format!(
                    r#"{XML_HEAD}
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree>{body}</p:spTree></p:cSld></p:sld>"#
                ),
            ));
        }
        let mut all: Vec<(String, String)> = vec![
            ("[Content_Types].xml".to_string(), "<Types/>".to_string()),
            (
                "ppt/presentation.xml".to_string(),
                format!(
                    r#"{XML_HEAD}
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>{sld_ids}</p:sldIdLst></p:presentation>"#
                ),
            ),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                format!(
                    r#"{XML_HEAD}
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
                ),
            ),
        ];
        all.extend(parts);
        let refs: Vec<(&str, &str)> = all
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_str()))
            .collect();
        zip_of(&refs)
    }

    /// テスト用の疑似乱数（依存を増やさないための線形合同法）
    struct Lcg(u64);

    impl Lcg {
        fn gen_u8(&mut self) -> u8 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            u8::try_from(self.0 >> 56).unwrap_or(0)
        }
    }

    #[test]
    fn mime_for_follows_the_extension_case_insensitively() {
        let cases = [
            ("a.png", "image/png"),
            ("a.PNG", "image/png"),
            ("dir/b.Jpg", "image/jpeg"),
            ("c.JPEG", "image/jpeg"),
            ("d.gif", "image/gif"),
            ("e.webp", "image/webp"),
            ("f.BMP", "image/bmp"),
            ("g.svg", "image/svg+xml"),
            ("h.pdf", "application/pdf"),
            (
                "i.DOCX",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "j.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                "k.Pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            ("l.txt", "application/octet-stream"),
            ("m.doc", "application/octet-stream"),
            ("noext", "application/octet-stream"),
            (".png", "application/octet-stream"),
            ("", "application/octet-stream"),
        ];
        for (name, expected) in cases {
            assert_eq!(mime_for(name), expected, "{name}");
        }
    }

    #[test]
    fn images_are_returned_as_image_by_extension_without_checking_content() {
        let bytes = b"\x89PNG\r\n\x1a\nbody".to_vec();
        assert_eq!(
            build_preview("photo.png", bytes.clone()),
            Preview::Image {
                mime: "image/png".to_string(),
                bytes: bytes.clone(),
            }
        );
        // 中身がテキストでも、拡張子で画像として返す（検証しない）
        assert_eq!(
            build_preview("Shot.JPG", b"not really".to_vec()),
            Preview::Image {
                mime: "image/jpeg".to_string(),
                bytes: b"not really".to_vec(),
            }
        );
        let svg = b"<svg><script>alert(1)</script></svg>".to_vec();
        assert_eq!(
            build_preview("logo.svg", svg.clone()),
            Preview::Image {
                mime: "image/svg+xml".to_string(),
                bytes: svg,
            }
        );
    }

    #[test]
    fn pdf_is_returned_as_pdf_by_extension() {
        let bytes = b"%PDF-1.4 minimal".to_vec();
        assert_eq!(
            build_preview("manual.PDF", bytes.clone()),
            Preview::Pdf { bytes }
        );
    }

    #[test]
    fn plain_text_of_any_extension_is_text() {
        for name in [
            "notes.txt",
            "README.md",
            "data.csv",
            "x.json",
            "app.log",
            "main.rs",
            "Makefile",
            "メモ.TXT",
        ] {
            assert_eq!(
                build_preview(name, "日本語 text\n".as_bytes().to_vec()),
                Preview::Text {
                    text: "日本語 text\n".to_string(),
                    truncated: false,
                },
                "{name}"
            );
        }
    }

    #[test]
    fn empty_file_is_empty_text() {
        assert_eq!(
            build_preview("empty.txt", Vec::new()),
            Preview::Text {
                text: String::new(),
                truncated: false,
            }
        );
        assert_eq!(
            build_preview("Empty", Vec::new()),
            Preview::Text {
                text: String::new(),
                truncated: false,
            }
        );
    }

    #[test]
    fn utf8_bom_is_removed() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice("こんにちは".as_bytes());
        assert_eq!(
            build_preview("bom.txt", bytes),
            Preview::Text {
                text: "こんにちは".to_string(),
                truncated: false,
            }
        );
        // BOM だけのファイルは空のテキスト
        assert_eq!(
            build_preview("bom_only.txt", UTF8_BOM.to_vec()),
            Preview::Text {
                text: String::new(),
                truncated: false,
            }
        );
    }

    #[test]
    fn nul_in_the_head_is_binary() {
        assert_eq!(
            build_preview("bin.txt", b"abc\0def".to_vec()),
            Preview::Unsupported
        );
        // 先頭 8 KiB の最後の位置にある NUL も検出する
        let mut edge = vec![b'a'; NUL_SCAN_BYTES];
        edge[NUL_SCAN_BYTES - 1] = 0;
        assert_eq!(build_preview("edge.log", edge), Preview::Unsupported);
    }

    #[test]
    fn nul_after_the_head_is_not_checked() {
        // 規則は「先頭 8 KiB に NUL が無ければテキスト」。それより後ろの NUL は検査の対象外（UTF-8 として正しければ表示する）
        let mut bytes = vec![b'a'; NUL_SCAN_BYTES];
        bytes.push(0);
        assert!(matches!(
            build_preview("late.txt", bytes),
            Preview::Text {
                truncated: false,
                ..
            }
        ));
    }

    #[test]
    fn invalid_utf8_is_unsupported() {
        assert_eq!(
            build_preview("bad.txt", vec![b'a', 0xff, 0xfe]),
            Preview::Unsupported
        );
        // 文字の途中で切れた UTF-8（末尾が壊れている）
        let cut = "あ".as_bytes()[..2].to_vec();
        assert_eq!(build_preview("cut.txt", cut), Preview::Unsupported);
    }

    #[test]
    fn text_is_truncated_at_one_mebibyte() {
        let exact = vec![b'a'; TEXT_MAX_BYTES];
        assert_eq!(
            build_preview("exact.txt", exact),
            Preview::Text {
                text: "a".repeat(TEXT_MAX_BYTES),
                truncated: false,
            }
        );
        let over = vec![b'a'; TEXT_MAX_BYTES + 1];
        match build_preview("over.txt", over) {
            Preview::Text { text, truncated } => {
                assert!(truncated);
                assert_eq!(text.len(), TEXT_MAX_BYTES);
            }
            other => panic!("テキストではありません: {other:?}"),
        }
    }

    #[test]
    fn truncation_keeps_multibyte_characters_whole() {
        // 「あ」は 3 バイト。1 MiB（1048576 バイト）はその途中に当たるため、直前の文字境界で切る
        let text = "あ".repeat(349_526);
        assert_eq!(text.len(), 1_048_578);
        match build_preview("jp.txt", text.into_bytes()) {
            Preview::Text { text, truncated } => {
                assert!(truncated);
                assert_eq!(text.len(), 1_048_575);
                assert_eq!(text.chars().count(), 349_525);
                assert!(text.chars().all(|c| c == 'あ'));
            }
            other => panic!("テキストではありません: {other:?}"),
        }
        // 4 バイトの文字（絵文字）でも同じ
        let emoji = "😀".repeat(262_144); // 1 MiB ちょうど（切れない）
        assert!(matches!(
            build_preview("emoji.txt", emoji.into_bytes()),
            Preview::Text {
                truncated: false,
                ..
            }
        ));
    }

    #[test]
    fn docx_becomes_one_section_of_paragraphs() {
        let bytes = docx_of(&["はじめに", "おわりに"]);
        assert_eq!(
            build_preview("report.DOCX", bytes),
            Preview::Document {
                format: DocFormat::Docx,
                sections: vec![DocSection {
                    title: None,
                    paragraphs: vec!["はじめに".to_string(), "おわりに".to_string()],
                    rows: None,
                }],
            }
        );
    }

    #[test]
    fn broken_office_files_are_unsupported() {
        // zip ではない
        assert_eq!(
            build_preview("bad.docx", b"not a zip".to_vec()),
            Preview::Unsupported
        );
        // zip だが本文の部品が無い（docx ではない）
        let no_body = zip_of(&[("[Content_Types].xml", "<Types/>")]);
        assert_eq!(
            build_preview("empty.docx", no_body.clone()),
            Preview::Unsupported
        );
        assert_eq!(
            build_preview("empty.xlsx", no_body.clone()),
            Preview::Unsupported
        );
        assert_eq!(build_preview("empty.pptx", no_body), Preview::Unsupported);
        // 本文の XML が壊れている（閉じタグの対応が合わない）
        let broken = zip_of(&[(
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p></w:body></w:document>"#,
        )]);
        assert_eq!(build_preview("broken.docx", broken), Preview::Unsupported);
    }

    #[test]
    fn xlsx_becomes_one_section_per_sheet_with_a_table() {
        let first = sheet_data(&[
            (0, 0, "名前"),
            (0, 1, "金額"),
            (0, 2, "備考"),
            (1, 0, "りんご"),
            (1, 1, "120"),
        ]);
        let empty = String::new();
        let bytes = xlsx_of(&[("一覧", first), ("空", empty)]);
        assert_eq!(
            build_preview("book.xlsx", bytes),
            Preview::Document {
                format: DocFormat::Xlsx,
                sections: vec![
                    DocSection {
                        title: Some("一覧".to_string()),
                        paragraphs: Vec::new(),
                        rows: Some(vec![
                            vec!["名前".to_string(), "金額".to_string(), "備考".to_string()],
                            vec!["りんご".to_string(), "120".to_string(), String::new()],
                        ]),
                    },
                    DocSection {
                        title: Some("空".to_string()),
                        paragraphs: Vec::new(),
                        rows: Some(Vec::new()),
                    },
                ],
            }
        );
    }

    #[test]
    fn xlsx_table_starts_at_a1_and_covers_the_max_row_and_column() {
        // 値は B3 だけにある。表は A1 を起点に 3 行 × 2 列になる
        let bytes = xlsx_of(&[("ずれ", sheet_data(&[(2, 1, "1.5")]))]);
        match build_preview("offset.xlsx", bytes) {
            Preview::Document { sections, .. } => {
                assert_eq!(
                    sections[0].rows,
                    Some(vec![
                        vec![String::new(), String::new()],
                        vec![String::new(), String::new()],
                        vec![String::new(), "1.5".to_string()],
                    ])
                );
            }
            other => panic!("文書ではありません: {other:?}"),
        }
    }

    #[test]
    fn xlsx_table_is_cut_at_500_rows_and_50_columns() {
        let data = sheet_data(&[
            (0, 0, "左上"),
            (499, 49, "境界"),
            (500, 0, "行の超過"),
            (0, 50, "列の超過"),
            (599, 79, "右下の超過"),
        ]);
        let bytes = xlsx_of(&[("大きい", data)]);
        match build_preview("big.xlsx", bytes) {
            Preview::Document { sections, .. } => {
                let rows = sections[0].rows.clone().expect("表がある");
                assert_eq!(rows.len(), SHEET_MAX_ROWS);
                assert!(rows.iter().all(|row| row.len() == SHEET_MAX_COLS));
                assert_eq!(rows[0][0], "左上");
                assert_eq!(rows[499][49], "境界");
                let all: Vec<&String> = rows.iter().flatten().collect();
                for cut in ["行の超過", "列の超過", "右下の超過"] {
                    assert!(!all.iter().any(|v| v.as_str() == cut), "{cut}");
                }
            }
            other => panic!("文書ではありません: {other:?}"),
        }
    }

    #[test]
    fn pptx_becomes_one_section_per_slide_in_presentation_order() {
        let bytes = pptx_of(&[&["一枚目の見出し", "本文"], &["二枚目"]]);
        assert_eq!(
            build_preview("deck.pptx", bytes),
            Preview::Document {
                format: DocFormat::Pptx,
                sections: vec![
                    DocSection {
                        title: Some("スライド 1".to_string()),
                        paragraphs: vec!["一枚目の見出し".to_string(), "本文".to_string()],
                        rows: None,
                    },
                    DocSection {
                        title: Some("スライド 2".to_string()),
                        paragraphs: vec!["二枚目".to_string()],
                        rows: None,
                    },
                ],
            }
        );
    }

    #[test]
    fn no_panic_on_arbitrary_bytes_and_names() {
        let names = ["a.docx", "a.xlsx", "a.pptx", "a.txt", "a.png", "a.pdf", "a"];
        let mut rng = Lcg(0x5eed);
        for name in names {
            for _ in 0..200 {
                let len = usize::from(rng.gen_u8()) * 4;
                let mut bytes: Vec<u8> = (0..len).map(|_| rng.gen_u8()).collect();
                // 一部は zip の先頭を付け、奥の解析まで進ませる
                if len % 2 == 0 && len >= 4 {
                    bytes[..4].copy_from_slice(b"PK\x03\x04");
                }
                let _ = build_preview(name, bytes);
            }
        }
    }

    #[test]
    fn no_panic_on_mutated_office_files() {
        // 正しい文書の 1 バイトずつを壊して、抽出の各段階で panic しないことを確かめる
        let samples = [
            ("m.docx", docx_of(&["abc"])),
            (
                "m.xlsx",
                xlsx_of(&[("S", sheet_data(&[(0, 0, "x"), (1, 1, "2")]))]),
            ),
            ("m.pptx", pptx_of(&[&["p"]])),
        ];
        for (name, original) in samples {
            for i in 0..original.len() {
                let mut bytes = original.clone();
                bytes[i] ^= 0xff;
                let _ = build_preview(name, bytes);
            }
            for cut in 0..original.len() {
                let _ = build_preview(name, original[..cut].to_vec());
            }
        }
    }
}
