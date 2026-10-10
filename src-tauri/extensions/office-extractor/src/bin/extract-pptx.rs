// pptx の抽出器（WASM）。入力は stdin の pptx のバイト列、出力は stdout の JSON。引数は取らない。
fn main() -> std::process::ExitCode {
    office_extractor::run(office_extractor::Kind::Pptx)
}
