// 安全上限（zip bomb・巨大入力への対策）。

const MIB: u64 = 1024 * 1024;

/// 抽出の上限。既定値は `Default` が返す
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 入力（docx・xlsx のファイル全体）の大きさ（バイト）
    pub max_input_bytes: u64,
    /// 展開後の合計（アーカイブ内のエントリを読んだバイト数の合計。圧縮前の大きさで数える）
    pub max_uncompressed_bytes: u64,
    /// zip のエントリ数
    pub max_entries: usize,
    /// xlsx の値のあるセル数（シートをまたいだ合計。空セルは数えない）
    pub max_cells: usize,
    /// docx の段落数（空の段落を含む）
    pub max_paragraphs: usize,
    /// xlsx のシート 1 枚の、セル参照の矩形（最小の行・列から最大の行・列まで）の面積。
    /// calamine は矩形の全体の領域を確保するため、遠くのセルを 1 つ書くだけで大きな確保になる。それを防ぐ
    pub max_sheet_area: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 50 * MIB,
            max_uncompressed_bytes: 100 * MIB,
            max_entries: 10_000,
            max_cells: 200_000,
            max_paragraphs: 100_000,
            max_sheet_area: 1_000_000,
        }
    }
}
