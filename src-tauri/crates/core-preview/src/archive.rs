// zip の読み取り（メモリ上だけ）。エントリ数と、読むバイト数の合計を上限で止める。
//
// - 展開先のファイルは作らない。エントリ名は `names` との完全一致の比較にだけ使う
// - 宣言された大きさ（中央ディレクトリ）と、実際に読んだ量の両方を確かめる。宣言は偽れるため、
//   読むときは残りの上限 + 1 バイトまでで止める

use crate::{Limits, PreviewError};
use std::io::{Cursor, Read};
use zip::result::ZipError;
use zip::ZipArchive;

/// 読み取り中の zip。`remaining` は、まだ読んでよい展開後の合計（バイト）
pub(crate) struct Archive<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    /// 各エントリの名前。インデックスは `ZipArchive` のインデックスと同じ
    names: Vec<String>,
    remaining: u64,
}

impl<'a> Archive<'a> {
    /// 入力の大きさ、zip の署名、エントリ数を確かめて開く
    pub(crate) fn open(bytes: &'a [u8], limits: &Limits) -> Result<Self, PreviewError> {
        if bytes.len() as u64 > limits.max_input_bytes {
            return Err(PreviewError::TooLarge);
        }
        // ローカルファイルヘッダ、空のアーカイブの終端、分割アーカイブの先頭だけを zip とみなす
        let is_zip = bytes.starts_with(b"PK\x03\x04")
            || bytes.starts_with(b"PK\x05\x06")
            || bytes.starts_with(b"PK\x07\x08");
        if !is_zip {
            return Err(PreviewError::NotZip);
        }
        let zip = ZipArchive::new(Cursor::new(bytes)).map_err(map_zip_error)?;
        if zip.len() > limits.max_entries {
            return Err(PreviewError::TooLarge);
        }
        let names = zip.file_names().map(str::to_owned).collect();
        Ok(Self {
            zip,
            names,
            remaining: limits.max_uncompressed_bytes,
        })
    }

    /// エントリ数
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// エントリの名前（`ZipArchive` のインデックスの順）
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    /// 名前が完全に一致するエントリのインデックス
    pub(crate) fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// 名前で指定したエントリを読む。無ければ None
    pub(crate) fn read_named(&mut self, name: &str) -> Result<Option<Vec<u8>>, PreviewError> {
        match self.index_of(name) {
            Some(index) => self.read_index(index).map(Some),
            None => Ok(None),
        }
    }

    /// インデックスのエントリを読む（残りの上限まで）。暗号化されたものは拒否する
    pub(crate) fn read_index(&mut self, index: usize) -> Result<Vec<u8>, PreviewError> {
        let (declared, encrypted) = {
            let raw = self.zip.by_index_raw(index).map_err(map_zip_error)?;
            (raw.size(), raw.encrypted())
        };
        if encrypted {
            return Err(PreviewError::Encrypted);
        }
        if declared > self.remaining {
            return Err(PreviewError::TooLarge);
        }
        let limit = self.remaining;
        let entry = self.zip.by_index(index).map_err(map_zip_error)?;
        let mut out = Vec::new();
        entry
            .take(limit.saturating_add(1))
            .read_to_end(&mut out)
            .map_err(|_| PreviewError::Corrupt)?;
        let read = out.len() as u64;
        if read > limit {
            return Err(PreviewError::TooLarge);
        }
        self.remaining -= read;
        Ok(out)
    }
}

/// zip のエラーを抽出の失敗へ写す。内容・エントリ名は含めない
fn map_zip_error(err: ZipError) -> PreviewError {
    match err {
        ZipError::InvalidPassword => PreviewError::Encrypted,
        ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED) => PreviewError::Encrypted,
        ZipError::UnsupportedArchive(_)
        | ZipError::CompressionMethodNotSupported(_)
        | ZipError::FileNotFound => PreviewError::Unsupported,
        ZipError::Io(_) | ZipError::InvalidArchive(_) => PreviewError::Corrupt,
        _ => PreviewError::Corrupt,
    }
}
