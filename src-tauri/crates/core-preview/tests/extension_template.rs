// 雛形の生成物（`scripts/create-extension.mjs` の出力を tests/fixtures/ にコミットしたもの）が、
// 拡張の登録の検証（マニフェスト・entry・権限・拡張子）を通ることを確かめる（設計書 11.3）。
// 雛形を変えたら、同じ内容を `node scripts/create-extension.mjs` で作り直して fixtures を更新する
// （scripts/create-extension.test.mjs が、生成物と fixtures の一致を確かめる）。
//
// 登録は `ExtensionHost` の公開 API（host の feature。既定で有効）で行う。
// 雛形は renderers だけを持つため、wasm は要らない。

#![cfg(feature = "host")]

use core_preview::{ExtensionHost, RenderMode};
use std::path::PathBuf;

/// 生成物の固定の場所（このテストのクレートの tests/fixtures）
fn template_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("extension-template")
}

#[test]
fn the_generated_template_registers_as_a_renderer() {
    let mut host = ExtensionHost::new();
    let id = host
        .register_dir(&template_dir())
        .expect("雛形は登録の検証を通る");
    assert_eq!(id.as_str(), "example.sample-preview");

    let renderer = host
        .renderer_for(".sample", RenderMode::View)
        .expect("雛形の拡張子の表示が選ばれる");
    assert_eq!(renderer.entry, "ui/index.html");
    assert_eq!(renderer.file_types, vec!["sample".to_owned()]);
    assert_eq!(renderer.modes, vec![RenderMode::View]);
}

#[test]
fn the_template_declares_no_permissions_and_no_extractors() {
    // 雛形は権限を持たず、抽出器も持たない（Rust の登録結果で確かめる）
    let mut host = ExtensionHost::new();
    host.register_dir(&template_dir())
        .expect("雛形は登録の検証を通る");
    assert!(host.extractors_for("sample").is_none());
}
