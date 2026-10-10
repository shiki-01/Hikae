// 比較の対象になる Office 文書の種類（拡張子で決める。中身は推測しない）。

use std::path::Path;

/// 抽出できる Office 文書の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    /// Word 文書（段落テキストを比べる）
    Docx,
    /// Excel ブック（シート別のセル値を比べる）
    Xlsx,
    /// PowerPoint プレゼンテーション（プレビューでスライドごとのテキストを出す。比較には使わない）
    Pptx,
}

impl OfficeKind {
    /// 拡張子（`.` を除いた英小文字）。拡張ホストで抽出器を探すときの語
    pub fn extension(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
        }
    }
}

/// ファイル名（相対パスでもよい）の拡張子から種類を決める。大文字小文字は区別しない。
/// 対応しない拡張子（`.doc`・`.docm`・`.xls` など）は None
pub fn office_kind(file_name: &str) -> Option<OfficeKind> {
    let ext = Path::new(file_name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" => Some(OfficeKind::Docx),
        "xlsx" => Some(OfficeKind::Xlsx),
        "pptx" => Some(OfficeKind::Pptx),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_follows_the_extension_case_insensitively() {
        assert_eq!(office_kind("第3章.docx"), Some(OfficeKind::Docx));
        assert_eq!(office_kind("dir/Book.XLSX"), Some(OfficeKind::Xlsx));
        assert_eq!(office_kind("old.doc"), None);
        assert_eq!(office_kind("macro.docm"), None);
        assert_eq!(office_kind("data.xls"), None);
        assert_eq!(office_kind("deck.PPTX"), Some(OfficeKind::Pptx));
        assert_eq!(office_kind("old.ppt"), None);
        assert_eq!(office_kind("noext"), None);
        assert_eq!(office_kind(".docx"), None);
    }
}
