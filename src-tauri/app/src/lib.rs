// Hikae の Tauri 層。コマンド・イベントの公開と、tauri-specta による TS 型の生成を行う。

use tauri_specta::{collect_commands, Builder};

/// 雛形確認用のサンプルコマンド（Phase 1 で削除する）
#[tauri::command]
#[specta::specta]
fn greet(name: String) -> String {
    format!("{name} さん、こんにちは")
}

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![greet])
}

/// src/lib/bindings.ts を書き出す（debug 起動時と export-bindings bin から呼ぶ）
pub fn export_bindings() {
    specta_builder()
        .export(
            specta_typescript::Typescript::default(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/bindings.ts"),
        )
        .expect("TS 型の書き出しに失敗しました");
}

pub fn run() {
    let builder = specta_builder();

    // debug ビルドでは起動のたびに TS 型を再生成する
    #[cfg(debug_assertions)]
    export_bindings();

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("アプリの起動に失敗しました");
}
