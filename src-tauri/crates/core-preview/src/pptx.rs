// pptx のスライドごとのテキストの抽出（プレビュー用。設計書 3.3 S4 の対象外で、表示だけに使う）。
//
// - スライドの順序は `ppt/presentation.xml` の `p:sldIdLst`（`r:id`）を、`ppt/_rels/presentation.xml.rels` の
//   Target で解決して決める。1 つでも解決できなければ、`ppt/slides/slideN.xml` の N の昇順にする
// - 対象は各スライドの部品（`ppt/slides/slideN.xml`）だけ。ノート・マスター・レイアウトは含めない
// - 段落（`a:p`）ごとに `a:t` の文字を出現順に連結する。`a:br` は空白 1 つ、`a:tab` はタブにする
// - 空（空白だけを含む）の段落は捨てる
// - 表のセル・グループ内の図形の段落も、出現順に他の段落と並べて含める
// - `mc:Fallback` の中身は含めない（docx と同じ）
// - 名前空間は URI で判定する。DTD・外部実体は読まない

use crate::archive::Archive;
use crate::{Limits, PreviewError};
use quick_xml::events::{BytesRef, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::{NsReader, XmlVersion};
use std::collections::HashMap;

/// PresentationML の名前空間（`p:sldId` の判定）
const P_NS: &[u8] = b"http://schemas.openxmlformats.org/presentationml/2006/main";
/// DrawingML の名前空間（段落 `a:p`・文字 `a:t` の判定）
const A_NS: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
/// 関係の名前空間（`r:id` 属性の判定）
const R_NS: &[u8] = b"http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Markup Compatibility の名前空間（`mc:Fallback` の判定）
const MC_NS: &[u8] = b"http://schemas.openxmlformats.org/markup-compatibility/2006";
/// スライドの並びを持つ部品
const PRESENTATION_PART: &str = "ppt/presentation.xml";
/// `PRESENTATION_PART` の関係
const PRESENTATION_RELS_PART: &str = "ppt/_rels/presentation.xml.rels";
/// 関係の Target を解決する基準のフォルダ（`presentation.xml` の置かれるフォルダ）
const PRESENTATION_DIR: &str = "ppt/";
/// スライドの部品の名前の接頭辞と接尾辞（`ppt/slides/slideN.xml`）
const SLIDE_PREFIX: &str = "ppt/slides/slide";
const SLIDE_SUFFIX: &str = ".xml";

/// スライド 1 枚。`index` は 1 始まりの表示順
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slide {
    /// 表示順（1 始まり）
    pub index: usize,
    /// 空でない段落のテキスト（出現順）
    pub paragraphs: Vec<String>,
}

/// pptx の全スライドを、表示順に返す。
///
/// スライドの部品が 1 つも無い（pptx ではない zip）ときは `Unsupported`。XML が壊れていれば `Corrupt`。
/// スライド数・段落数が上限を超えれば `TooLarge`。
pub fn extract_pptx(bytes: &[u8], limits: &Limits) -> Result<Vec<Slide>, PreviewError> {
    let mut archive = Archive::open(bytes, limits)?;
    let parts = slide_parts(&mut archive, limits)?;
    if parts.is_empty() {
        return Err(PreviewError::Unsupported);
    }
    if parts.len() > limits.max_slides {
        return Err(PreviewError::TooLarge);
    }
    let mut paragraph_count = 0usize;
    let mut slides = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let xml = archive.read_named(part)?.ok_or(PreviewError::Corrupt)?;
        let paragraphs = parse_slide(&xml, &mut paragraph_count, limits.max_paragraphs)?;
        slides.push(Slide {
            index: i + 1,
            paragraphs,
        });
    }
    Ok(slides)
}

/// 表示順のスライドの部品名。presentation.xml の並びで解決できればそれを、できなければ番号順を使う
fn slide_parts(archive: &mut Archive<'_>, limits: &Limits) -> Result<Vec<String>, PreviewError> {
    if let Some(parts) = parts_by_presentation(archive, limits)? {
        return Ok(parts);
    }
    Ok(parts_by_slide_number(archive))
}

/// presentation.xml の `sldIdLst` を、関係の Target を通して部品名へ解決する。
/// 1 つでも解決できなければ None（呼び出し側が番号順に切り替える）。上限の超過だけは伝える
fn parts_by_presentation(
    archive: &mut Archive<'_>,
    limits: &Limits,
) -> Result<Option<Vec<String>>, PreviewError> {
    let Some(presentation) = archive.read_named(PRESENTATION_PART)? else {
        return Ok(None);
    };
    let Some(rels) = archive.read_named(PRESENTATION_RELS_PART)? else {
        return Ok(None);
    };
    // 壊れた XML は解決できないものとして、番号順に任せる
    let ids = match slide_ids(&presentation, limits.max_slides) {
        Ok(ids) => ids,
        Err(PreviewError::TooLarge) => return Err(PreviewError::TooLarge),
        Err(_) => return Ok(None),
    };
    let targets = match relationship_targets(&rels) {
        Ok(targets) => targets,
        Err(PreviewError::TooLarge) => return Err(PreviewError::TooLarge),
        Err(_) => return Ok(None),
    };
    if ids.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::with_capacity(ids.len());
    for id in &ids {
        let Some(target) = targets.get(id) else {
            return Ok(None);
        };
        let part = resolve_target(target);
        if archive.index_of(&part).is_none() {
            return Ok(None);
        }
        parts.push(part);
    }
    Ok(Some(parts))
}

/// `p:sldId` の `r:id` を、出現順に集める。上限を超えたら `TooLarge`
fn slide_ids(xml: &[u8], max: usize) -> Result<Vec<String>, PreviewError> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut ids = Vec::new();
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
        if !is_ns(&ns, P_NS) || element.local_name().as_ref() != b"sldId" {
            continue;
        }
        let mut id = None;
        for attr in element.attributes() {
            let attr = attr.map_err(|_| PreviewError::Corrupt)?;
            let (attr_ns, local) = reader.resolver().resolve_attribute(attr.key);
            if is_ns(&attr_ns, R_NS) && local.as_ref() == b"id" {
                let value = attr
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                    .map_err(|_| PreviewError::Corrupt)?
                    .into_owned();
                id = Some(value);
            }
        }
        // 関係の無いスライドは解決できないため、呼び出し側が番号順に切り替える
        let id = id.ok_or(PreviewError::Corrupt)?;
        if ids.len() >= max {
            return Err(PreviewError::TooLarge);
        }
        ids.push(id);
    }
    Ok(ids)
}

/// 関係の Id → Target の対応（外部のリンクは含めない）
fn relationship_targets(xml: &[u8]) -> Result<HashMap<String, String>, PreviewError> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut targets = HashMap::new();
    loop {
        buf.clear();
        let (_, event) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(|_| PreviewError::Corrupt)?;
        let element = match event {
            Event::Start(e) | Event::Empty(e) => e,
            Event::Eof => break,
            _ => continue,
        };
        if element.local_name().as_ref() != b"Relationship" {
            continue;
        }
        let mut id = None;
        let mut target = None;
        let mut external = false;
        for attr in element.attributes() {
            let attr = attr.map_err(|_| PreviewError::Corrupt)?;
            let value = attr
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| PreviewError::Corrupt)?
                .into_owned();
            match attr.key.as_ref() {
                b"Id" => id = Some(value),
                b"Target" => target = Some(value),
                b"TargetMode" => external = value == "External",
                _ => {}
            }
        }
        if let (Some(id), Some(target), false) = (id, target, external) {
            targets.insert(id, target);
        }
    }
    Ok(targets)
}

/// 関係の Target を、zip の中の部品名にする。先頭の `/` は zip の根から、それ以外は presentation.xml の
/// フォルダから解決する。`.` と `..` を畳む（zip の外へは出ない。外へ出る分は捨てる）
fn resolve_target(target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => format!("{PRESENTATION_DIR}{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// 番号順のスライドの部品。`ppt/slides/slideN.xml`（N は 10 進の数字だけ）を N の昇順に並べる
fn parts_by_slide_number(archive: &Archive<'_>) -> Vec<String> {
    let mut numbered: Vec<(u64, String)> = archive
        .names()
        .iter()
        .filter_map(|name| {
            let digits = name
                .strip_prefix(SLIDE_PREFIX)?
                .strip_suffix(SLIDE_SUFFIX)?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((digits.parse::<u64>().ok()?, name.clone()))
        })
        .collect();
    numbered.sort();
    // 同じ番号の名前が並ぶ zip は、先頭のものだけを使う（読み取りは名前で行うため）
    numbered.dedup_by_key(|(number, _)| *number);
    numbered.into_iter().map(|(_, name)| name).collect()
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
        ResolveResult::Bound(Namespace(uri)) if *uri == A_NS => match local {
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

/// 名前空間が一致するか
fn is_ns(ns: &ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(ns, ResolveResult::Bound(Namespace(uri)) if *uri == expected)
}

/// 1 枚のスライドの部品を解析し、空でない段落を出現順に返す。
/// `counter` は全スライドの段落数（上限の判定用）
fn parse_slide(
    xml: &[u8],
    counter: &mut usize,
    max_paragraphs: usize,
) -> Result<Vec<String>, PreviewError> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // 開いている段落の文字（段落の外では None）
    let mut current: Option<String> = None;
    // `a:t` の中にいる深さ
    let mut text_depth = 0usize;
    // `mc:Fallback` の中にいる深さ（中身は読まない）
    let mut fallback_depth = 0usize;
    loop {
        buf.clear();
        let (ns, event) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(|_| PreviewError::Corrupt)?;
        match event {
            Event::Start(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => fallback_depth += 1,
                    _ if fallback_depth > 0 => {}
                    Tag::Paragraph => current = Some(String::new()),
                    Tag::Text => text_depth += 1,
                    Tag::Tab => push_str(&mut current, "\t"),
                    Tag::Break => push_str(&mut current, " "),
                    Tag::Other => {}
                }
            }
            Event::Empty(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => {}
                    _ if fallback_depth > 0 => {}
                    // 空の段落（`<a:p/>`）は捨てるため、何もしない
                    Tag::Paragraph | Tag::Text | Tag::Other => {}
                    Tag::Tab => push_str(&mut current, "\t"),
                    Tag::Break => push_str(&mut current, " "),
                }
            }
            Event::End(e) => {
                let tag = classify(&ns, e.local_name().as_ref());
                match tag {
                    Tag::Fallback => fallback_depth = fallback_depth.saturating_sub(1),
                    _ if fallback_depth > 0 => {}
                    Tag::Paragraph => {
                        if let Some(text) = current.take() {
                            emit(&mut out, text, counter, max_paragraphs)?;
                        }
                    }
                    Tag::Text => text_depth = text_depth.saturating_sub(1),
                    Tag::Tab | Tag::Break | Tag::Other => {}
                }
            }
            Event::Text(t) => {
                if text_depth > 0 && fallback_depth == 0 {
                    let text = t.decode().map_err(|_| PreviewError::Corrupt)?;
                    push_str(&mut current, &text);
                }
            }
            Event::CData(c) => {
                if text_depth > 0 && fallback_depth == 0 {
                    let text = c.decode().map_err(|_| PreviewError::Corrupt)?;
                    push_str(&mut current, &text);
                }
            }
            Event::GeneralRef(r) => {
                if text_depth > 0 && fallback_depth == 0 {
                    let text = reference_text(&r)?;
                    push_str(&mut current, &text);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // 閉じられていない段落が残るのは、XML として壊れている
    if current.is_some() {
        return Err(PreviewError::Corrupt);
    }
    Ok(out)
}

/// 開いている段落へ文字を足す。段落の外では捨てる
fn push_str(current: &mut Option<String>, text: &str) {
    if let Some(buf) = current.as_mut() {
        buf.push_str(text);
    }
}

/// 空白だけの段落は捨てる。残すものは上限の数に数える
fn emit(
    out: &mut Vec<String>,
    text: String,
    counter: &mut usize,
    max_paragraphs: usize,
) -> Result<(), PreviewError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    if *counter >= max_paragraphs {
        return Err(PreviewError::TooLarge);
    }
    *counter += 1;
    out.push(text);
    Ok(())
}

/// 文字参照（`&amp;` や `&#x41;`）を文字列にする。定義されていない実体は空にする（docx と同じ）
fn reference_text(reference: &BytesRef<'_>) -> Result<String, PreviewError> {
    if let Some(c) = reference
        .resolve_char_ref()
        .map_err(|_| PreviewError::Corrupt)?
    {
        return Ok(c.to_string());
    }
    let name = reference.decode().map_err(|_| PreviewError::Corrupt)?;
    Ok(match name.as_ref() {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => "",
    }
    .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{pptx_of, pptx_para, pptx_presentation, pptx_rels, pptx_slide};

    /// 段落の文字だけを、表示順に並べたもの
    fn texts(slides: &[Slide]) -> Vec<Vec<String>> {
        slides.iter().map(|s| s.paragraphs.clone()).collect()
    }

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn slides_follow_the_slide_id_list_not_the_part_names() -> Result<(), PreviewError> {
        let zip = pptx_of(&[
            ("ppt/presentation.xml", pptx_presentation(&["rId2", "rId1"])),
            (
                "ppt/_rels/presentation.xml.rels",
                pptx_rels(&[("rId1", "slides/slide1.xml"), ("rId2", "slides/slide2.xml")]),
            ),
            ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("first"))),
            ("ppt/slides/slide2.xml", pptx_slide(&pptx_para("second"))),
        ]);
        let slides = extract_pptx(&zip, &limits())?;
        assert_eq!(
            slides.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(texts(&slides), vec![vec!["second"], vec!["first"]]);
        Ok(())
    }

    #[test]
    fn absolute_targets_are_resolved_from_the_package_root() -> Result<(), PreviewError> {
        let zip = pptx_of(&[
            ("ppt/presentation.xml", pptx_presentation(&["rId1"])),
            (
                "ppt/_rels/presentation.xml.rels",
                pptx_rels(&[("rId1", "/ppt/slides/slide7.xml")]),
            ),
            ("ppt/slides/slide7.xml", pptx_slide(&pptx_para("seven"))),
        ]);
        assert_eq!(texts(&extract_pptx(&zip, &limits())?), vec![vec!["seven"]]);
        Ok(())
    }

    #[test]
    fn break_is_a_space_and_tab_is_a_tab() -> Result<(), PreviewError> {
        let body = r#"<a:p><a:r><a:t>a</a:t></a:r><a:br/><a:r><a:t>b</a:t></a:r><a:r><a:t>c</a:t><a:tab/><a:t>d</a:t></a:r></a:p>"#;
        let zip = pptx_of(&[("ppt/slides/slide1.xml", pptx_slide(body))]);
        // `a:br` は空白 1 つ、`a:tab` はタブ 1 つ（間の `a:r` は連結するだけ）
        assert_eq!(
            texts(&extract_pptx(&zip, &limits())?),
            vec![vec!["a bc\td"]]
        );
        Ok(())
    }

    #[test]
    fn empty_and_blank_paragraphs_are_dropped_and_empty_slides_stay() -> Result<(), PreviewError> {
        let body = format!(
            "<a:p/>{}<a:p><a:r><a:t>   </a:t></a:r></a:p>{}",
            pptx_para("kept"),
            pptx_para("\u{3000}")
        );
        let zip = pptx_of(&[
            ("ppt/slides/slide1.xml", pptx_slide(&body)),
            ("ppt/slides/slide2.xml", pptx_slide("")),
        ]);
        let slides = extract_pptx(&zip, &limits())?;
        assert_eq!(texts(&slides), vec![vec!["kept"], vec![]]);
        assert_eq!(slides[1].index, 2);
        Ok(())
    }

    #[test]
    fn paragraphs_inside_tables_are_included_in_document_order() -> Result<(), PreviewError> {
        let table = format!(
            r#"<a:tbl><a:tr><a:tc><a:txBody><a:bodyPr/>{}</a:txBody></a:tc><a:tc><a:txBody><a:bodyPr/>{}</a:txBody></a:tc></a:tr></a:tbl>"#,
            pptx_para("c1"),
            pptx_para("c2")
        );
        let zip = pptx_of(&[(
            "ppt/slides/slide1.xml",
            pptx_slide(&format!("{}{}", pptx_para("before"), table)),
        )]);
        assert_eq!(
            texts(&extract_pptx(&zip, &limits())?),
            vec![vec!["before", "c1", "c2"]]
        );
        Ok(())
    }

    #[test]
    fn notes_and_masters_are_not_included() -> Result<(), PreviewError> {
        let zip = pptx_of(&[
            ("ppt/presentation.xml", pptx_presentation(&["rId1"])),
            (
                "ppt/_rels/presentation.xml.rels",
                pptx_rels(&[("rId1", "slides/slide1.xml")]),
            ),
            ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("body"))),
            (
                "ppt/notesSlides/notesSlide1.xml",
                pptx_slide(&pptx_para("note")),
            ),
            (
                "ppt/slideMasters/slideMaster1.xml",
                pptx_slide(&pptx_para("master")),
            ),
        ]);
        assert_eq!(texts(&extract_pptx(&zip, &limits())?), vec![vec!["body"]]);
        Ok(())
    }

    #[test]
    fn without_relationships_slides_follow_the_numeric_order_of_their_names(
    ) -> Result<(), PreviewError> {
        let zip = pptx_of(&[
            ("ppt/slides/slide10.xml", pptx_slide(&pptx_para("ten"))),
            ("ppt/slides/slide2.xml", pptx_slide(&pptx_para("two"))),
            ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("one"))),
        ]);
        assert_eq!(
            texts(&extract_pptx(&zip, &limits())?),
            vec![vec!["one"], vec!["two"], vec!["ten"]]
        );
        Ok(())
    }

    #[test]
    fn an_unresolved_relationship_falls_back_to_the_numeric_order() -> Result<(), PreviewError> {
        let zip = pptx_of(&[
            ("ppt/presentation.xml", pptx_presentation(&["rId9", "rId1"])),
            (
                "ppt/_rels/presentation.xml.rels",
                pptx_rels(&[("rId1", "slides/slide1.xml")]),
            ),
            ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("one"))),
            ("ppt/slides/slide2.xml", pptx_slide(&pptx_para("two"))),
        ]);
        assert_eq!(
            texts(&extract_pptx(&zip, &limits())?),
            vec![vec!["one"], vec!["two"]]
        );
        Ok(())
    }

    #[test]
    fn text_is_unescaped() -> Result<(), PreviewError> {
        let zip = pptx_of(&[(
            "ppt/slides/slide1.xml",
            pptx_slide(&pptx_para("A &amp; B &lt;x&gt; &#x41;")),
        )]);
        assert_eq!(
            texts(&extract_pptx(&zip, &limits())?),
            vec![vec!["A & B <x> A"]]
        );
        Ok(())
    }

    #[test]
    fn a_zip_without_slides_is_unsupported() {
        let zip = pptx_of(&[("word/document.xml", "<w/>".to_owned())]);
        assert_eq!(
            extract_pptx(&zip, &limits()),
            Err(PreviewError::Unsupported)
        );
    }

    #[test]
    fn a_non_zip_input_is_not_zip() {
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        bytes.extend_from_slice(&[0u8; 64]);
        assert_eq!(extract_pptx(&bytes, &limits()), Err(PreviewError::NotZip));
    }

    #[test]
    fn a_broken_zip_is_corrupt() {
        let mut bytes = b"PK\x03\x04".to_vec();
        bytes.extend_from_slice(&[0xFF; 40]);
        assert_eq!(extract_pptx(&bytes, &limits()), Err(PreviewError::Corrupt));
    }

    #[test]
    fn broken_slide_xml_is_corrupt() {
        let zip = pptx_of(&[(
            "ppt/slides/slide1.xml",
            pptx_slide("<a:p><a:r><a:t>x</a:p>"),
        )]);
        assert_eq!(extract_pptx(&zip, &limits()), Err(PreviewError::Corrupt));
    }

    #[test]
    fn more_slides_than_the_limit_is_too_large() {
        let zip = pptx_of(&[
            ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("a"))),
            ("ppt/slides/slide2.xml", pptx_slide(&pptx_para("b"))),
        ]);
        let limits = Limits {
            max_slides: 1,
            ..Limits::default()
        };
        assert_eq!(extract_pptx(&zip, &limits), Err(PreviewError::TooLarge));
    }

    #[test]
    fn more_paragraphs_than_the_limit_is_too_large() {
        let zip = pptx_of(&[(
            "ppt/slides/slide1.xml",
            pptx_slide(&format!("{}{}", pptx_para("a"), pptx_para("b"))),
        )]);
        let limits = Limits {
            max_paragraphs: 1,
            ..Limits::default()
        };
        assert_eq!(extract_pptx(&zip, &limits), Err(PreviewError::TooLarge));
    }
}
