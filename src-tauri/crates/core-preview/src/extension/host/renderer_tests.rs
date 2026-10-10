// 表示（renderer）の宣言の登録と選択のテスト（設計書 11.2）。抽出器のテストは tests.rs にある。

use super::*;
use serde_json::json;

/// 一時フォルダに拡張を作る（マニフェストと、フォルダ直下のファイル）
fn folder(manifest: &serde_json::Value, files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join(MANIFEST_FILE), manifest.to_string())
        .unwrap_or_else(|e| panic!("write: {e}"));
    for name in files {
        std::fs::write(dir.path().join(name), b"<!doctype html>")
            .unwrap_or_else(|e| panic!("write: {e}"));
    }
    dir
}

/// 表示の宣言だけを持つマニフェスト
fn manifest(id: &str, renderers: serde_json::Value) -> serde_json::Value {
    json!({
        "id": id,
        "name": "テスト",
        "version": "1.0.0",
        "apiVersion": "1",
        "contributes": { "renderers": renderers }
    })
}

#[test]
fn an_exact_file_type_wins_over_the_star_whatever_the_order() {
    // 全部の拡張子（`*`）を先に登録しても、`.png` の宣言が選ばれる
    let mut host = ExtensionHost::new();
    let star = folder(
        &manifest(
            "test.star",
            json!([{ "fileTypes": ["*"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    let png = folder(
        &manifest(
            "test.png",
            json!([{ "fileTypes": [".png"], "modes": ["view"], "entry": "png.html" }]),
        ),
        &["png.html"],
    );
    host.register_dir(star.path())
        .unwrap_or_else(|e| panic!("{e}"));
    host.register_dir(png.path())
        .unwrap_or_else(|e| panic!("{e}"));

    let exact = host.renderer_for("png", RenderMode::View);
    assert_eq!(exact.map(|r| r.entry.as_str()), Some("png.html"));
    // 拡張子の表記（`.` の有無、大文字）は問わない
    assert_eq!(
        host.renderer_for(".PNG", RenderMode::View)
            .map(|r| r.entry.as_str()),
        Some("png.html")
    );
    // 宣言の無い拡張子は `*` に落ちる
    assert_eq!(
        host.renderer_for("pdf", RenderMode::View)
            .map(|r| r.extension_id.as_str()),
        Some("test.star")
    );
    // 対応しない方法（compare）は、どれも返さない
    assert!(host.renderer_for("png", RenderMode::Compare).is_none());
}

#[test]
fn the_same_file_type_and_mode_claimed_twice_is_rejected() {
    let mut host = ExtensionHost::new();
    let first = folder(
        &manifest(
            "test.first",
            json!([{ "fileTypes": [".png"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    host.register_dir(first.path())
        .unwrap_or_else(|e| panic!("{e}"));

    // 別の拡張が同じ拡張子・同じ方法を扱うと拒否し、登録は増えない
    let clash = folder(
        &manifest(
            "test.clash",
            json!([{ "fileTypes": [".png"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    assert_eq!(
        host.register_dir(clash.path()).err(),
        Some(ManifestError::FileTypeConflict)
    );
    assert_eq!(host.info().len(), 1);
    assert_eq!(host.renderers().len(), 1);

    // 同じ拡張子でも、方法が違えば共存できる
    let compare = folder(
        &manifest(
            "test.compare",
            json!([{ "fileTypes": [".png"], "modes": ["compare"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    assert!(host.register_dir(compare.path()).is_ok());
}

#[test]
fn two_star_renderers_conflict_and_a_duplicate_mode_in_one_manifest_is_rejected() {
    let mut host = ExtensionHost::new();
    let one = folder(
        &manifest(
            "test.one",
            json!([{ "fileTypes": ["*"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    host.register_dir(one.path())
        .unwrap_or_else(|e| panic!("{e}"));
    let two = folder(
        &manifest(
            "test.two",
            json!([{ "fileTypes": ["*"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    assert_eq!(
        host.register_dir(two.path()).err(),
        Some(ManifestError::FileTypeConflict)
    );

    // 1 つの拡張の中で同じ拡張子・方法が 2 回出てくる場合も拒否する
    let mut fresh = ExtensionHost::new();
    let twice = folder(
        &manifest(
            "test.twice",
            json!([
                { "fileTypes": [".png"], "modes": ["view"], "entry": "index.html" },
                { "fileTypes": [".png", ".jpg"], "modes": ["view"], "entry": "index.html" }
            ]),
        ),
        &["index.html"],
    );
    assert_eq!(
        fresh.register_dir(twice.path()).err(),
        Some(ManifestError::FileTypeConflict)
    );
    assert!(fresh.renderers().is_empty());
}

#[test]
fn a_renderer_entry_must_exist_inside_the_folder() {
    let mut host = ExtensionHost::new();
    let missing = folder(
        &manifest(
            "test.missing",
            json!([{ "fileTypes": ["*"], "modes": ["view"], "entry": "absent.html" }]),
        ),
        &[],
    );
    assert_eq!(
        host.register_dir(missing.path()).err(),
        Some(ManifestError::EntryMissing)
    );
    assert!(host.renderers().is_empty());
}

#[test]
fn a_registered_folder_is_the_root_for_asset_serving_and_unknown_ids_have_none() {
    let mut host = ExtensionHost::new();
    let dir = folder(
        &manifest(
            "test.root",
            json!([{ "fileTypes": ["*"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    host.register_dir(dir.path())
        .unwrap_or_else(|e| panic!("{e}"));
    let root = host
        .extension_root("test.root")
        .unwrap_or_else(|| panic!("登録済みの拡張の根が無い"));
    assert!(root.join("index.html").is_file());
    assert!(host.extension_root("test.none").is_none());
}

#[test]
fn a_renderer_only_extension_has_an_empty_extractor_list() {
    // 表示だけの拡張でも登録でき、抽出器は使われない
    let mut host = ExtensionHost::new();
    let dir = folder(
        &manifest(
            "test.view-only",
            json!([{ "fileTypes": [".png"], "modes": ["view"], "entry": "index.html" }]),
        ),
        &["index.html"],
    );
    host.register_dir(dir.path())
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(host.extractors_for("png").is_none());
    assert_eq!(host.renderers()[0].modes, vec![RenderMode::View]);
}
