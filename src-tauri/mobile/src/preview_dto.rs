// core-remote の `Preview` を、画面に返す `MobilePreview` へ変換する。

use base64::Engine;
use core_remote::preview::{DocFormat as CrFormat, Preview};

use crate::dto::{DocFormat, DocSection, MobilePreview};

pub fn to_dto(p: Preview) -> MobilePreview {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    match p {
        Preview::Text { text, truncated } => MobilePreview::Text { text, truncated },
        Preview::Image { mime, bytes } => MobilePreview::Image {
            mime,
            content_base64: b64(&bytes),
        },
        Preview::Pdf { bytes } => MobilePreview::Pdf {
            content_base64: b64(&bytes),
        },
        Preview::Document { format, sections } => MobilePreview::Document {
            format: match format {
                CrFormat::Docx => DocFormat::Docx,
                CrFormat::Xlsx => DocFormat::Xlsx,
                CrFormat::Pptx => DocFormat::Pptx,
            },
            sections: sections
                .into_iter()
                .map(|s| DocSection {
                    title: s.title,
                    paragraphs: s.paragraphs,
                    rows: s.rows,
                })
                .collect(),
        },
        Preview::Unsupported => MobilePreview::Unsupported,
    }
}
