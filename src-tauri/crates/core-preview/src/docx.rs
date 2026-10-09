// docx の段落テキストの抽出（設計書 3.3 S4）。書式は無視し、文字の内容だけを比べるための段落列を作る。
//
// - 対象は `word/document.xml` だけ（ヘッダー・フッター・脚注・注釈は含めない）
// - 段落（`w:p`）ごとに、`w:t` の文字を出現順に連結する
// - `w:tab` はタブ、`w:br` は改行ではなく空白 1 つにする
// - 書式・画像・図形の見た目は無視する。表のセルの段落も、出現順に他の段落と並べて含める
// - 空の段落も残す（段落の追加・削除を差分で見るため）
// - 変更履歴の削除された文字（`w:delText`）は含めない（最終的に残る文字だけを比べる）
// - `mc:Fallback` の中身は含めない（同じ内容を二重に数えないため。`mc:Choice` 側を使う）
// - 図形の中の段落（`w:txbxContent`）は、本文の段落と同じく出現順に含める
// - 名前空間は URI で判定する（接頭辞 `w:` は約束ではない）。DTD・外部実体は読まない

use crate::archive::Archive;
use crate::{Limits, PreviewError};
use quick_xml::events::{BytesRef, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::NsReader;

/// WordprocessingML の名前空間
const W_NS: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// Markup Compatibility の名前空間（`mc:Fallback` の判定）
const MC_NS: &[u8] = b"http://schemas.openxmlformats.org/markup-compatibility/2006";
/// 本文の部品の名前
const DOCUMENT_PART: &str = "word/document.xml";

/// docx の本文の段落を、出現順のテキストとして返す。
///
/// 本文の部品が無い（docx ではない zip）ときは `Unsupported`。XML が壊れていれば `Corrupt`。
pub fn extract_docx(bytes: &[u8], limits: &Limits) -> Result<Vec<String>, PreviewError> {
    let mut archive = Archive::open(bytes, limits)?;
    let xml = archive
        .read_named(DOCUMENT_PART)?
        .ok_or(PreviewError::Unsupported)?;
    parse_document(&xml, limits.max_paragraphs)
}

/// XML の要素の種類（抽出に関係するものだけ）
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Paragraph,
    Text,
    Tab,
    Break,
    Fallback,
    Other,
}

/// 名前空間と局所名から要素の種類を決める
fn classify(ns: &ResolveResult<'_>, local: &[u8]) -> Tag {
    match ns {
        ResolveResult::Bound(Namespace(uri)) if *uri == W_NS => match local {
            b"p" => Tag::Paragraph,
            b"t" => Tag::Text,
            b"tab" => Tag::Tab,
            b"br" => Tag::Break,
            _ => Tag::Other,
        },
        ResolveResult::Bound(Namespace(uri)) if *uri == MC_NS && local == b"Fallback" => {
            Tag::Fallback
        }
        _ => Tag::Other,
    }
}

/// 段落を集める途中の状態
struct Document {
    /// 完了した段落（出現順）
    paragraphs: Vec<String>,
    /// 開いている段落（図形の中の段落に備えて、入れ子を積む）
    pending: Vec<String>,
    /// 段落の上限
    max: usize,
    /// `w:t` の中にいる深さ
    text_depth: usize,
    /// `mc:Fallback` の中にいる深さ（中身は読まない）
    fallback_depth: usize,
}

impl Document {
    fn new(max: usize) -> Self {
        Self {
            paragraphs: Vec::new(),
            pending: Vec::new(),
            max,
            text_depth: 0,
            fallback_depth: 0,
        }
    }

    fn begin_paragraph(&mut self) {
        self.pending.push(String::new());
    }

    /// 開いている段落を閉じて、出現順に加える
    fn end_paragraph(&mut self) -> Result<(), PreviewError> {
        match self.pending.pop() {
            Some(text) => self.emit(text),
            None => Ok(()),
        }
    }

    /// 空の段落（`<w:p/>`）を加える
    fn empty_paragraph(&mut self) -> Result<(), PreviewError> {
        self.emit(String::new())
    }

    fn emit(&mut self, text: String) -> Result<(), PreviewError> {
        if self.paragraphs.len() >= self.max {
            return Err(PreviewError::TooLarge);
        }
        self.paragraphs.push(text);
        Ok(())
    }

    /// いちばん内側の開いている段落へ文字を足す。段落の外では捨てる
    fn push_str(&mut self, text: &str) {
        if let Some(top) = self.pending.last_mut() {
            top.push_str(text);
        }
    }

    fn push_char(&mut self, c: char) {
        if let Some(top) = self.pending.last_mut() {
            top.push(c);
        }
    }

    /// 文字参照（`&amp;` や `&#x41;`）を文字にする。定義されていない実体は無視する
    fn push_reference(&mut self, reference: &BytesRef<'_>) -> Result<(), PreviewError> {
        if let Some(c) = reference
            .resolve_char_ref()
            .map_err(|_| PreviewError::Corrupt)?
        {
            self.push_char(c);
            return Ok(());
        }
        let name = reference.decode().map_err(|_| PreviewError::Corrupt)?;
        match name.as_ref() {
            "lt" => self.push_char('<'),
            "gt" => self.push_char('>'),
            "amp" => self.push_char('&'),
            "apos" => self.push_char('\''),
            "quot" => self.push_char('"'),
            _ => {}
        }
        Ok(())
    }

    /// 本文の中（`mc:Fallback` の外）にいて、文字を受け取ってよいか
    fn accepts_text(&self) -> bool {
        self.text_depth > 0 && self.fallback_depth == 0
    }
}

/// `word/document.xml` の段落を集める
fn parse_document(xml: &[u8], max_paragraphs: usize) -> Result<Vec<String>, PreviewError> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut doc = Document::new(max_paragraphs);
    loop {
        buf.clear();
        let (ns, event) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(|_| PreviewError::Corrupt)?;
        match event {
            Event::Start(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => doc.fallback_depth += 1,
                    _ if doc.fallback_depth > 0 => {}
                    Tag::Paragraph => doc.begin_paragraph(),
                    Tag::Text => doc.text_depth += 1,
                    Tag::Tab => doc.push_char('\t'),
                    Tag::Break => doc.push_char(' '),
                    Tag::Other => {}
                }
            }
            Event::Empty(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => {}
                    _ if doc.fallback_depth > 0 => {}
                    Tag::Paragraph => doc.empty_paragraph()?,
                    Tag::Tab => doc.push_char('\t'),
                    Tag::Break => doc.push_char(' '),
                    Tag::Text | Tag::Other => {}
                }
            }
            Event::End(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => doc.fallback_depth = doc.fallback_depth.saturating_sub(1),
                    _ if doc.fallback_depth > 0 => {}
                    Tag::Paragraph => doc.end_paragraph()?,
                    Tag::Text => doc.text_depth = doc.text_depth.saturating_sub(1),
                    Tag::Tab | Tag::Break | Tag::Other => {}
                }
            }
            Event::Text(t) => {
                if doc.accepts_text() {
                    let text = t.decode().map_err(|_| PreviewError::Corrupt)?;
                    doc.push_str(&text);
                }
            }
            Event::CData(c) => {
                if doc.accepts_text() {
                    let text = c.decode().map_err(|_| PreviewError::Corrupt)?;
                    doc.push_str(&text);
                }
            }
            Event::GeneralRef(r) => {
                if doc.accepts_text() {
                    doc.push_reference(&r)?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // 閉じられていない段落が残るのは、XML として壊れている
    if !doc.pending.is_empty() {
        return Err(PreviewError::Corrupt);
    }
    Ok(doc.paragraphs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{document, docx_of, para, zip_of};

    fn limits() -> Limits {
        Limits::default()
    }

    /// 本文の XML を docx にして抽出する
    fn extract(body: &str) -> Result<Vec<String>, PreviewError> {
        extract_docx(&docx_of(&document(body)), &limits())
    }

    #[test]
    fn each_paragraph_becomes_one_string_in_order() -> Result<(), PreviewError> {
        let body = format!("{}{}", para("first"), para("second"));
        assert_eq!(extract(&body)?, vec!["first", "second"]);
        Ok(())
    }

    #[test]
    fn runs_inside_a_paragraph_are_concatenated() -> Result<(), PreviewError> {
        let body =
            r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hel</w:t></w:r><w:r><w:t>lo</w:t></w:r></w:p>"#;
        // 書式（太字）は無視し、文字だけを連結する
        assert_eq!(extract(body)?, vec!["Hello"]);
        Ok(())
    }

    #[test]
    fn empty_paragraphs_are_kept_in_both_forms() -> Result<(), PreviewError> {
        let body = format!("{}<w:p/><w:p></w:p>{}", para("a"), para("b"));
        assert_eq!(extract(&body)?, vec!["a", "", "", "b"]);
        Ok(())
    }

    #[test]
    fn tab_becomes_a_tab_and_break_becomes_one_space() -> Result<(), PreviewError> {
        let body = r#"<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>"#;
        assert_eq!(extract(body)?, vec!["a\tb c"]);
        Ok(())
    }

    #[test]
    fn paragraphs_inside_table_cells_are_included_in_order() -> Result<(), PreviewError> {
        let table = format!(
            r#"<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>"#,
            para("cell1"),
            para("cell2")
        );
        let body = format!("{}{}{}", para("before"), table, para("after"));
        assert_eq!(extract(&body)?, vec!["before", "cell1", "cell2", "after"]);
        Ok(())
    }

    #[test]
    fn xml_escapes_and_character_references_are_decoded() -> Result<(), PreviewError> {
        let body = r#"<w:p><w:r><w:t>A &amp; B &lt;x&gt; &quot;q&quot; &apos;a&apos; &#x41;&#66;</w:t></w:r></w:p>"#;
        assert_eq!(extract(body)?, vec!["A & B <x> \"q\" 'a' AB"]);
        Ok(())
    }

    #[test]
    fn deleted_text_and_fallback_content_are_not_counted() -> Result<(), PreviewError> {
        let body = r#"<w:p><w:del><w:r><w:delText>gone</w:delText></w:r></w:del><w:r><w:t>kept</w:t></w:r></w:p>
<w:p><mc:AlternateContent><mc:Choice Requires="wps"><w:r><w:t>choice</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>fallback</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>"#;
        assert_eq!(extract(body)?, vec!["kept", "choice"]);
        Ok(())
    }

    #[test]
    fn a_namespace_prefix_alone_is_not_what_decides_a_paragraph() -> Result<(), PreviewError> {
        // 別の名前空間の `p` と `t` は段落にも文字にもならない
        let body =
            r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><a:p xmlns:a="urn:other"><a:t>other</a:t></a:p>"#;
        assert_eq!(extract(body)?, vec!["x"]);
        Ok(())
    }

    #[test]
    fn a_zip_without_the_body_part_is_unsupported() {
        let zip = zip_of(&[("xl/workbook.xml", b"<workbook/>")], false);
        assert_eq!(
            extract_docx(&zip, &limits()),
            Err(PreviewError::Unsupported)
        );
    }

    #[test]
    fn a_compound_file_is_not_a_zip() {
        // 旧形式の .doc（OLE 複合ファイル）の先頭
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        bytes.extend_from_slice(&[0u8; 64]);
        assert_eq!(extract_docx(&bytes, &limits()), Err(PreviewError::NotZip));
        assert_eq!(extract_docx(&[], &limits()), Err(PreviewError::NotZip));
    }

    #[test]
    fn a_broken_zip_is_corrupt() {
        let mut bytes = b"PK\x03\x04".to_vec();
        bytes.extend_from_slice(&[0xFF; 40]);
        assert_eq!(extract_docx(&bytes, &limits()), Err(PreviewError::Corrupt));

        // 正しい zip を途中で切ったもの（中央ディレクトリが無い）
        let full = docx_of(&document(&para("x")));
        assert_eq!(
            extract_docx(&full[..full.len() / 2], &limits()),
            Err(PreviewError::Corrupt)
        );
    }

    #[test]
    fn broken_xml_is_corrupt() {
        assert_eq!(
            extract(r#"<w:p><w:r><w:t>x</w:r></w:p>"#),
            Err(PreviewError::Corrupt)
        );
    }

    #[test]
    fn an_unclosed_paragraph_is_corrupt() {
        // 段落が閉じないまま本文が終わる
        let xml = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:body></w:document>"#;
        assert_eq!(
            extract_docx(&docx_of(xml), &limits()),
            Err(PreviewError::Corrupt)
        );
    }

    #[test]
    fn an_encrypted_entry_is_refused_before_it_is_read() {
        let mut bytes = docx_of(&document(&para("secret")));
        // 中央ディレクトリの全項目の汎用フラグの暗号化ビット（bit 0）を立てる
        let mut i = 0;
        while i + 10 <= bytes.len() {
            if &bytes[i..i + 4] == b"PK\x01\x02" {
                bytes[i + 8] |= 0x01;
            }
            i += 1;
        }
        assert_eq!(
            extract_docx(&bytes, &limits()),
            Err(PreviewError::Encrypted)
        );
    }

    #[test]
    fn the_input_size_limit_is_checked_first() {
        let bytes = docx_of(&document(&para("x")));
        let small = Limits {
            max_input_bytes: 10,
            ..Limits::default()
        };
        assert_eq!(extract_docx(&bytes, &small), Err(PreviewError::TooLarge));
    }

    #[test]
    fn too_many_entries_is_too_large() {
        let zip = zip_of(
            &[("a", b"1"), ("b", b"2"), ("word/document.xml", b"<x/>")],
            false,
        );
        let small = Limits {
            max_entries: 2,
            ..Limits::default()
        };
        assert_eq!(extract_docx(&zip, &small), Err(PreviewError::TooLarge));
    }

    #[test]
    fn too_many_paragraphs_is_too_large() {
        let body = format!("{}{}{}", para("1"), para("2"), para("3"));
        let small = Limits {
            max_paragraphs: 2,
            ..Limits::default()
        };
        assert_eq!(
            extract_docx(&docx_of(&document(&body)), &small),
            Err(PreviewError::TooLarge)
        );
    }

    #[test]
    fn a_declared_size_over_the_budget_is_too_large() {
        let bytes = docx_of(&document(&para(&"x".repeat(5000))));
        let small = Limits {
            max_uncompressed_bytes: 1000,
            ..Limits::default()
        };
        assert_eq!(extract_docx(&bytes, &small), Err(PreviewError::TooLarge));
    }

    #[test]
    fn a_highly_compressed_bomb_stops_at_the_uncompressed_limit() {
        // 20 MiB の同じ文字を圧縮すると、数十 KiB にしかならない
        let text = "a".repeat(20 * 1024 * 1024);
        let xml = document(&format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#));
        let zip = zip_of(&[("word/document.xml", xml.as_bytes())], true);
        assert!(zip.len() < 1024 * 1024, "the fixture must be small on disk");
        let small = Limits {
            max_uncompressed_bytes: 1024 * 1024,
            ..Limits::default()
        };
        assert_eq!(extract_docx(&zip, &small), Err(PreviewError::TooLarge));
        // 上限を十分に大きくすれば、同じものが読める
        let large = Limits {
            max_uncompressed_bytes: 64 * 1024 * 1024,
            ..Limits::default()
        };
        let paragraphs = extract_docx(&zip, &large);
        assert_eq!(paragraphs.map(|p| p.len()), Ok(1));
    }

    #[test]
    fn entry_names_are_only_compared_and_never_used_as_paths() -> Result<(), PreviewError> {
        // 名前にパストラバーサルがあっても、本文の部品と完全に一致しなければ読まない（書きもしない）
        let zip = zip_of(
            &[
                ("../word/document.xml", b"<x/>"),
                ("word/document.xml", document(&para("ok")).as_bytes()),
            ],
            false,
        );
        assert_eq!(extract_docx(&zip, &limits())?, vec!["ok"]);
        Ok(())
    }
}
