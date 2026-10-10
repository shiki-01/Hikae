// 実機の llama-server と GGUF を使う統合テスト。通常の `cargo test` では実行しない（#[ignore]）。
//
// 実行例（PowerShell 以外の環境でも同じ環境変数を設定する）:
//   HIKAE_TEST_LLAMA_SERVER=<llama-server の実行ファイル> HIKAE_TEST_MODEL=<GGUF のパス> \
//   cargo test -p core-llm --test real_llama -- --ignored --nocapture
//
// モデルの出力は確率的（temperature 0.2）なので、内容ではなく「検証を通る日本語 1 文が返ること」を確かめる。
// 導入フローは通さず（カタログのフォルダ配置を使わない）、サイドカー・生成・検証を直接つないで確かめる。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    build_messages, clean_summary, ChangeInput, ChangeKind, ChangedFile, ChatClient, LlamaServer,
    LlamaServerOptions,
};
use std::path::PathBuf;
use std::time::Duration;

fn input(files: Vec<ChangedFile>) -> ChangeInput {
    ChangeInput { files }
}

/// 評価レポートの入力に近い 3 件（docx の段落追加、xlsx のセル変更、ファイル削除）
fn sample_inputs() -> Vec<(&'static str, ChangeInput)> {
    vec![
        (
            "docx 段落の追加",
            input(vec![ChangedFile {
                kind: ChangeKind::Modified,
                name: "第3章_実験.docx".into(),
                old_name: None,
                amount: Some("段落 +2 / -0".into()),
                diff_text: Some(
                    "+ 温度を30度にした条件で追加実験を行った。\n+ 反応速度は1.8倍に増加した。".into(),
                ),
            }]),
        ),
        (
            "xlsx セルの変更",
            input(vec![ChangedFile {
                kind: ChangeKind::Modified,
                name: "売上管理.xlsx".into(),
                old_name: None,
                amount: Some("セル 3 件".into()),
                diff_text: Some(
                    "シート「2024年度」\n  B4: 120000 -> 135000\n  B5: 98000 -> 87000\n  B12: 842000 -> 846000".into(),
                ),
            }]),
        ),
        (
            "削除のみ",
            input(vec![
                ChangedFile {
                    kind: ChangeKind::Deleted,
                    name: "古い資料/会議メモ_2023.docx".into(),
                    old_name: None,
                    amount: None,
                    diff_text: None,
                },
                ChangedFile {
                    kind: ChangeKind::Deleted,
                    name: "古い資料/見積_旧.xlsx".into(),
                    old_name: None,
                    amount: None,
                    diff_text: None,
                },
            ]),
        ),
    ]
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要（環境変数 HIKAE_TEST_LLAMA_SERVER、HIKAE_TEST_MODEL）"]
async fn real_llama_server_returns_valid_sentence() {
    let (Some(exe), Some(model)) = (
        std::env::var_os("HIKAE_TEST_LLAMA_SERVER"),
        std::env::var_os("HIKAE_TEST_MODEL"),
    ) else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    let model = PathBuf::from(model);
    assert!(model.is_file(), "モデルファイルがありません");

    let server = LlamaServer::new(LlamaServerOptions::new(Some(PathBuf::from(exe)))).unwrap();
    let base = server.ensure_started(&model).await.unwrap();
    let chat = ChatClient::new().unwrap();

    // 全件の結果を出してから判定する（途中で止めると、残りの出力を確認できないため）
    let mut rejected = Vec::new();
    for (title, input) in sample_inputs() {
        let messages = build_messages(&input);
        let raw = chat
            .chat(&base, "model", &messages, Duration::from_secs(30))
            .await
            .unwrap();
        let cleaned = clean_summary(&raw);
        println!("[{title}] raw: {raw}");
        println!("[{title}] cleaned: {cleaned:?}");
        if cleaned.is_none() {
            rejected.push(title);
        }
    }

    server.stop().await;
    assert!(!server.is_running().await);
    assert!(
        rejected.is_empty(),
        "検証を通る 1 文ではなかった入力: {rejected:?}"
    );
}
