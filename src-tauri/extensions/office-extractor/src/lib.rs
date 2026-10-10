// 組み込みの Office 抽出器（WASM 版、設計書 11.1）。3 つの bin（extract-docx・extract-xlsx・extract-pptx）が使う。
//
// - stdin の入力（文書のバイト列）を読み、対応する `core_preview::extract_*` の結果を stdout に JSON で書く
// - 出力の形式は `core_preview::extension` の規約（paragraphs・cells・pages）
// - 成功は終了コード 0。失敗は終了コード 1 と `{"error": "<語彙>"}`（上限・形式・暗号化・破損）
// - ファイル・環境変数・引数・ネットワーク・時計は使わない（ホストが与えない）。stderr には何も書かない

use core_preview::extension::{error_json, ExtractOutput};
use core_preview::{extract_docx, extract_pptx, extract_xlsx, Limits, PreviewError};
use std::io::{Read, Write};
use std::process::ExitCode;

/// 抽出する文書の種類（bin ごとに 1 つ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Xlsx,
    Pptx,
}

/// stdin を読んで抽出し、結果を stdout に書く
pub fn run(kind: Kind) -> ExitCode {
    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        return ExitCode::FAILURE;
    }
    // 上限はネイティブの既定値をそのまま使う（ホストと同じ判定にするため）
    let limits = Limits::default();
    let result: Result<ExtractOutput, PreviewError> = match kind {
        Kind::Docx => extract_docx(&input, &limits).map(ExtractOutput::Paragraphs),
        Kind::Xlsx => extract_xlsx(&input, &limits).map(ExtractOutput::Cells),
        Kind::Pptx => extract_pptx(&input, &limits).map(ExtractOutput::Pages),
    };
    match result {
        Ok(output) => match output.to_json() {
            Ok(json) if write_stdout(&json) => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        },
        Err(err) => {
            // 失敗の語彙は stdout に書く。書けなくても終了コードで失敗は伝わる
            write_stdout(&error_json(err));
            ExitCode::FAILURE
        }
    }
}

/// stdout に書いて流す。成功したかを返す
fn write_stdout(bytes: &[u8]) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes).and_then(|()| out.flush()).is_ok()
}
