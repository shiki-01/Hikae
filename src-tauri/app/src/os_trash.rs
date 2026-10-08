// OS のごみ箱への移動（core-ops の `Trasher` の実装）。
//
// 新規ファイルの「元に戻す（作成しない）」（設計書 4.2）が、ファイルを完全には削除せず、
// ごみ箱へ移すために使う。ごみ箱が使えない環境（無効化されている、権限がないなど）では
// `trash` crate がエラーを返すだけでファイルは残る。呼び出し側（core-ops）がそれを
// 「削除せずエラー」として扱う。

use core_ops::Trasher;
use std::io;
use std::path::Path;

/// OS のごみ箱（Windows のごみ箱、macOS の「ゴミ箱」）へ移す
pub(crate) struct OsTrash;

impl Trasher for OsTrash {
    fn trash(&self, path: &Path) -> io::Result<()> {
        // `trash` crate は、親フォルダを解決してから対象を移す（最後の成分のリンクはたどらない）。
        // Windows の verbatim 形式（`\\?\` 付き）のパスは、crate の内部でシェルが受け付ける形に直される
        trash::delete(path).map_err(|e| io::Error::other(e.to_string()))
    }
}
