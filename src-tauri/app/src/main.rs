// Release ビルドで Windows のコンソールウィンドウを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // git から credential helper として呼ばれた場合（`hikae credential get` など）は、
    // 画面を起動せずに応答だけ返して終了する
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("credential") {
        std::process::exit(hikae_lib::run_credential_helper(args.next().as_deref()));
    }
    hikae_lib::run();
}
