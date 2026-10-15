// Hikae モバイル版（iOS / Android）の Tauri アプリ。設計書 14 章。
// git は使わず、GitHub REST API（core-remote）でリモートのリポジトリを閲覧し、ファイルを追加する。

// Tauri のコマンドは Result<_, MobileError> を返す形に決まっている（app と同じ扱い）
#![allow(clippy::result_large_err)]

mod commands;
mod dto;
mod error;
mod preview_dto;
mod tokens;

use commands::MobileState;
use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::mobile_account,
        commands::mobile_start_login,
        commands::mobile_open_login_page,
        commands::mobile_wait_login,
        commands::mobile_cancel_login,
        commands::mobile_logout,
        commands::mobile_list_repos,
        commands::mobile_list_dir,
        commands::mobile_preview,
        commands::mobile_download,
        commands::mobile_file_history,
        commands::mobile_plan_upload,
        commands::mobile_upload,
    ])
}

/// src/lib/bindings-mobile.ts を書き出す（export-bindings-mobile bin と、デスクトップの debug 起動時に呼ぶ）。
/// 一時ファイルに生成し、既存の内容と同じなら書き換えない（Vite の HMR を余計に動かさないため）
pub fn export_bindings() {
    let dest = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/lib/bindings-mobile.ts"
    ));
    let tmp = std::env::temp_dir().join(format!("hikae-bindings-mobile-{}.ts", std::process::id()));
    specta_builder()
        .export(specta_typescript::Typescript::default(), &tmp)
        .expect("TS 型の書き出しに失敗しました");
    let generated = std::fs::read(&tmp).expect("生成した TS 型の読み込みに失敗しました");
    let _ = std::fs::remove_file(&tmp);
    if std::fs::read(dest).ok().as_deref() != Some(generated.as_slice()) {
        std::fs::write(dest, &generated).expect("TS 型の書き出しに失敗しました");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    let invoke_handler = builder.invoke_handler();

    #[cfg(all(debug_assertions, desktop))]
    export_bindings();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_hikae_secure_store::init())
        .setup(|app| {
            app.manage(MobileState::new(app.handle().clone()));
            // CI のシミュレータ確認用: 環境変数があるときだけ、トークンの保管（Keychain）の読み書きを確かめて結果を標準出力へ出す
            if std::env::var_os("HIKAE_SELFTEST").is_some() {
                let tokens = app.state::<MobileState>().tokens();
                tauri::async_runtime::spawn(async move {
                    match tokens.selftest().await {
                        Ok(result) => println!("HIKAE_SELFTEST keychain={result}"),
                        Err(error) => println!("HIKAE_SELFTEST keychain=FAILED {error}"),
                    }
                });
            }
            // 画面は、このフラグを見て `/m`（モバイルの画面）へ移る（設計書 14.3）
            let window =
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                    .initialization_script("window.__HIKAE_MOBILE__ = true;");
            // デスクトップ上で動かすときだけ、スマホの画面に近い大きさにする
            #[cfg(desktop)]
            let window = window.title("Hikae").inner_size(430.0, 860.0);
            window.build()?;
            Ok(())
        })
        .invoke_handler(invoke_handler)
        .run(tauri::generate_context!())
        .expect("Tauri アプリの起動に失敗しました");
}
