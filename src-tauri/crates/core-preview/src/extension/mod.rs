// 拡張 API（設計書 11 章）の共通部分。
//
// - `output`: 抽出器の出力の形（JSON）。WASM 抽出器（extensions/office-extractor）も使うため、常に含まれる
// - `host`: 拡張の登録と WASM の実行（wasmtime）。feature `host`（既定で有効）のときだけ含まれる
//
// 抽出器はファイルシステム・ネットワーク・環境変数・引数・時計に触れられない（host で何も与えない）。
// 入力は stdin のバイト列、出力は stdout の JSON。

pub mod output;

#[cfg(feature = "host")]
pub mod host;

pub use output::{
    decode_error, decode_output, error_from_word, error_json, error_word, DecodeError,
    ExtractOutput, OutputKind,
};
