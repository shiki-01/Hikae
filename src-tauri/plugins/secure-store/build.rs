// コマンドは画面（JS）に公開しない（Rust からだけ呼ぶ）ため、コマンド一覧は空。
// Android のプロジェクト（android/）を Tauri のビルドに登録する。
fn main() {
    tauri_plugin::Builder::new(&[]).android_path("android").build();
}
