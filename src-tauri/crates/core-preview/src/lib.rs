// ファイルプレビュー（設計書 3.3 S4 の docx / xlsx 比較、8.3）。docx の段落テキスト、xlsx のセルの値を取り出し、
// pptx のスライドごとのテキストを取り出す（pptx は表示のみ。比較の対象ではない）。
// 2 つの版の差分を作る。Tauri・core-git・他の core-* に依存しない純粋なライブラリ。
// 拡張ホスト（`extension`、設計書 11 章）は feature `host` で含まれ、wasmtime で WASM の抽出器を実行する。
// WASM の抽出器（extensions/office-extractor）は feature を切ってこのクレートを使い、同じ抽出のコードを持つ。
//
// - 入力は呼び出し側が読んだバイト列。ファイルシステムへは一切書かない（zip の中身はメモリ上でだけ読む）
// - zip のエントリ名（パストラバーサルを含みうる）は、名前の完全一致の比較にだけ使う
// - 外部エンティティ・DTD は展開しない（quick-xml の既定。未知の実体参照は無視する）
// - 上限（`Limits`）を超える入力は、解析を続けずに `PreviewError::TooLarge` で止める

mod archive;
mod diff;
mod docx;
mod error;
pub mod extension;
mod kind;
mod limits;
mod pptx;
mod xlsx;

pub use diff::{cell_ref, diff_paragraphs, diff_sheets, CellChange, ParagraphDiff, SheetsDiff};
pub use docx::extract_docx;
pub use error::PreviewError;
#[cfg(feature = "host")]
pub use extension::host::{
    ExtensionHost, ExtensionId, ExtensionInfo, ExtractError, ExtractorRef, ManifestError,
    RuntimeLimits,
};
pub use extension::{ExtractOutput, OutputKind};
pub use kind::{office_kind, OfficeKind};
pub use limits::Limits;
pub use pptx::{extract_pptx, Slide};
pub use xlsx::{extract_xlsx, Sheet};

#[cfg(test)]
pub(crate) mod test_support {
    //! テスト用の最小の docx・xlsx を、メモリ上の zip として組み立てる（実在の Office は使わない）

    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    /// 名前と中身の組から zip を作る。`deflate` が真なら圧縮して書く
    pub(crate) fn zip_of(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let method = if deflate {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        };
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, data) in entries {
            writer.start_file(*name, options).expect("start_file");
            writer.write_all(data).expect("write_all");
        }
        writer.finish().expect("finish").into_inner()
    }

    /// 名前と中身の組から pptx 相当の zip を作る（メモリ上だけ）
    pub(crate) fn pptx_of(parts: &[(&str, String)]) -> Vec<u8> {
        let entries: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (*n, d.as_bytes())).collect();
        zip_of(&entries, true)
    }

    /// スライドの部品の XML（本文を `p:spTree` の中に入れる）
    pub(crate) fn pptx_slide(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree>{body}</p:spTree></p:cSld></p:sld>"#
        )
    }

    /// 1 段落（`a:p`）の XML
    pub(crate) fn pptx_para(text: &str) -> String {
        format!(r#"<a:p><a:r><a:t>{text}</a:t></a:r></a:p>"#)
    }

    /// 関係の Id を順に並べた `presentation.xml`
    pub(crate) fn pptx_presentation(rel_ids: &[&str]) -> String {
        let items: String = rel_ids
            .iter()
            .enumerate()
            .map(|(i, id)| format!(r#"<p:sldId id="{}" r:id="{id}"/>"#, 256 + i))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>{items}</p:sldIdLst></p:presentation>"#
        )
    }

    /// `(Id, Target)` の組から `presentation.xml.rels`
    pub(crate) fn pptx_rels(pairs: &[(&str, &str)]) -> String {
        let items: String = pairs
            .iter()
            .map(|(id, target)| {
                format!(
                    r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="{target}"/>"#
                )
            })
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{items}</Relationships>"#
        )
    }

    /// `word/document.xml` だけを持つ docx 相当の zip
    pub(crate) fn docx_of(document_xml: &str) -> Vec<u8> {
        zip_of(
            &[
                ("[Content_Types].xml", b"<Types/>"),
                ("word/document.xml", document_xml.as_bytes()),
            ],
            false,
        )
    }

    /// 本文を `<w:document>` で包む
    pub(crate) fn document(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>{body}</w:body></w:document>"#
        )
    }

    /// 1 段落（`w:p`）の XML
    pub(crate) fn para(text: &str) -> String {
        format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
    }

    /// シート名と `<sheetData>` の中身の組から xlsx 相当の zip を作る（圧縮あり）
    pub(crate) fn xlsx_of(sheets: &[(&str, &str)]) -> Vec<u8> {
        let mut workbook_sheets = String::new();
        let mut rels = String::new();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for (i, (name, data)) in sheets.iter().enumerate() {
            let n = i + 1;
            workbook_sheets.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            ));
            let sheet = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
            );
            entries.push((format!("xl/worksheets/sheet{n}.xml"), sheet.into_bytes()));
        }
        let workbook = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{workbook_sheets}</sheets></workbook>"#
        );
        let rels_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
        );
        let package_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
        let mut all: Vec<(String, Vec<u8>)> = vec![
            ("[Content_Types].xml".to_string(), b"<Types/>".to_vec()),
            ("_rels/.rels".to_string(), package_rels.as_bytes().to_vec()),
            ("xl/workbook.xml".to_string(), workbook.into_bytes()),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                rels_xml.into_bytes(),
            ),
        ];
        all.extend(entries);
        let refs: Vec<(&str, &[u8])> = all
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        zip_of(&refs, true)
    }

    /// 数値のセル
    pub(crate) fn num(cell: &str, value: &str) -> String {
        format!(r#"<c r="{cell}"><v>{value}</v></c>"#)
    }

    /// inlineStr の文字列のセル
    pub(crate) fn text(cell: &str, value: &str) -> String {
        format!(r#"<c r="{cell}" t="inlineStr"><is><t>{value}</t></is></c>"#)
    }

    /// `<row>` で包む
    pub(crate) fn row(number: u32, cells: &str) -> String {
        format!(r#"<row r="{number}">{cells}</row>"#)
    }
}
