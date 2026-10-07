// Device Flow の完全な例。
// 使用方法:
//   - client_id を設定: $env:HIKAE_GITHUB_CLIENT_ID = "..." (PowerShell) または export HIKAE_GITHUB_CLIENT_ID=... (Bash)
//   - 実行: cargo run -p core-github --example device_flow
//   - トークンを保存: cargo run -p core-github --example device_flow -- --save
//   - 保存されたトークンを削除: cargo run -p core-github --example device_flow -- --delete
//   - 保存状態を確認: cargo run -p core-github --example device_flow -- --load

use core_github::{client_id_from_env, DeviceFlowClient, TokenStore, UserClient};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // 引数をチェック
    if args.len() > 1 {
        match args[1].as_str() {
            "--delete" => {
                println!("保存されたトークンを削除しています...");
                TokenStore::delete()?;
                println!("トークンを削除しました。");
                return Ok(());
            }
            "--load" => {
                println!("保存状態を確認しています...");
                let exists = TokenStore::exists()?;
                if exists {
                    println!("トークンが保存されています。");
                } else {
                    println!("トークンは保存されていません。");
                }
                return Ok(());
            }
            "--save" => {
                // フラグは後で処理
            }
            _ => {
                eprintln!("不明なオプション: {}", args[1]);
                eprintln!("使用方法:");
                eprintln!(
                    "  {}               - Device Flow で認証（トークンを保存しない）",
                    args[0]
                );
                eprintln!(
                    "  {} --save         - Device Flow で認証してトークンを保存",
                    args[0]
                );
                eprintln!("  {} --load         - 保存状態を確認", args[0]);
                eprintln!("  {} --delete       - 保存されたトークンを削除", args[0]);
                return Ok(());
            }
        }
    }

    let save_token = args.len() > 1 && args[1] == "--save";

    // client_id を環境変数から読み込む
    let client_id = client_id_from_env()?;

    println!("GitHub OAuth Device Flow で認証を開始します...\n");

    // Device Flow クライアントを作成
    let device_flow = DeviceFlowClient::new(client_id);

    // Device code を要求
    println!("Device code を取得しています...");
    let device = device_flow.request_device_code().await?;

    println!("\n=== 認証手順 ===");
    println!("1. 以下の URL にアクセスしてください:");
    println!("   {}", device.verification_uri);
    println!("\n2. 以下のコードを入力してください:");
    println!("   {}", device.user_code);
    println!("\n3. 認証後、このプログラムで自動的にトークンを取得します。");
    println!("\n有効期限: {} 秒\n", device.expires_in);

    println!("トークンを取得するためにポーリングしています...");
    println!("（Ctrl+C で中止できます）\n");

    // トークンを取得するまでポーリング
    let token = device_flow.poll_for_token(&device, None).await?;

    println!("\n認証が成功しました。");

    // ユーザー情報を取得
    println!("ユーザー情報を取得しています...");
    let user_client = UserClient::new();
    let user = user_client.fetch_user(&token).await?;

    println!("\nログイン情報:");
    println!("  ユーザー名: {}", user.login);
    println!("  ID: {}", user.id);
    println!("  noreply メール: {}", user.noreply_email());

    // トークンを保存する場合
    if save_token {
        println!("\nトークンを OS キーチェーンに保存しています...");
        TokenStore::save(&token)?;
        println!("トークンを保存しました。");
    }

    println!("\n認証が完了しました。");

    Ok(())
}
