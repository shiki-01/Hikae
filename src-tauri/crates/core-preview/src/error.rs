// 抽出・差分の失敗。メッセージには内容・パス・エントリ名を含めない。

use std::fmt;

/// 抽出の失敗の種類（画面は種類に応じた文言を出す）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewError {
    /// 上限を超えた（入力の大きさ、展開後の合計、エントリ数、段落数、セル数、矩形の面積）
    TooLarge,
    /// zip ではない（旧形式の .doc・.xls、OLE 形式の暗号化ファイルを含む）
    NotZip,
    /// zip のエントリが暗号化されている
    Encrypted,
    /// 対応していない内容（docx・xlsx の主要な部品が無い、対応しない圧縮方式など）
    Unsupported,
    /// 壊れている（zip の構造、XML の構文、圧縮データ、セル参照）
    Corrupt,
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::TooLarge => "preview input exceeds a safety limit",
            Self::NotZip => "input is not a zip container",
            Self::Encrypted => "zip entry is encrypted",
            Self::Unsupported => "document content is not supported",
            Self::Corrupt => "document is corrupt",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PreviewError {}
