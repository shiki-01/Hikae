// 実機の llama-server と GGUF を使う統合テスト。通常の `cargo test` では実行しない（#[ignore]）。
//
// 実行例（環境変数は実行前に設定する）:
//   HIKAE_TEST_LLAMA_SERVER=<llama-server の実行ファイル> HIKAE_TEST_MODEL=<GGUF のパス>
//   cargo test -p core-llm --test real_llama -- --ignored --nocapture
//
// モデルの出力は確率的（temperature 0.2）なので、内容ではなく「検証を通る日本語 1 文が返ること」を確かめる。
// 導入フローは通さず（カタログのフォルダ配置を使わない）、サイドカー・生成・検証を直接つないで確かめる。
//
// 計測（`measure_suggestion_inputs`）は、保存メモの入力の型ごとに同じ回数だけ生成し、
// 検証の通過率（救済を含む・含まない）、所要時間、入力のトークン数を表で出す。
// 追加の環境変数: HIKAE_TEST_RUNS（既定 8）、HIKAE_TEST_ONLY（例: 6 で入力 6 だけ）、
// HIKAE_TEST_THREADS（llama-server の -t。未指定はアプリと同じ既定値）。
// 入力はテスト内で作る（合成データ。リポジトリに大きなファイルを置かない）。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    build_messages, cell_line, clean_summary, clean_summary_detail, ChangeInput, ChangeKind,
    ChangedFile, ChatClient, LlamaServer, LlamaServerOptions, GENERATE_TIMEOUT,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn input(files: Vec<ChangedFile>) -> ChangeInput {
    ChangeInput { files }
}

fn file(kind: ChangeKind, name: &str, amount: &str, diff: Option<String>) -> ChangedFile {
    ChangedFile {
        kind,
        name: name.into(),
        old_name: None,
        amount: Some(amount.into()),
        diff_text: diff,
    }
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

/// 計測用の入力（設計の入力の型どおり。差分は「+ 本文」「Sheet1!B4: 旧 → 新」の形）
fn measurement_inputs() -> Vec<(&'static str, ChangeInput)> {
    // 1: ソース 1 ファイル 500 行追加
    let case1 = input(vec![file(
        ChangeKind::Modified,
        "src/report_builder.rs",
        "行 +500 / -0",
        Some(
            (0..500)
                .map(|i| format!("+ let value_{i} = build_row({i}, \"列{i}\");"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    )]);
    // 2: ソース 3 ファイル約 300 行
    let case2 = input(vec![
        file(
            ChangeKind::Modified,
            "src/parser.rs",
            "行 +100 / -0",
            Some(
                (0..100)
                    .map(|i| format!("+ fn parse_{i}(s: &str) -> Option<u32> {{ s.parse().ok() }}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "src/ui_handler.ts",
            "行 +100 / -2",
            Some(
                (0..100)
                    .map(|i| format!("+ export function handle{i}(e: Event) {{ return e; }}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Added,
            "tools/config.py",
            "行 +100 / -0",
            Some(
                (0..100)
                    .map(|i| format!("+ KEY_{i} = \"value-{i}\""))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
    ]);
    // 3: 日本語 md 800 行追加
    let case3 = input(vec![file(
        ChangeKind::Modified,
        "docs/第3章_考察.md",
        "行 +800 / -0",
        Some(
            (0..800)
                .map(|i| {
                    format!(
                        "+ 実験{i}回目では、温度を{}度に設定し、反応速度を記録した。",
                        30 + i % 20
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    )]);
    // 4: docx 60 段落＋xlsx 40 セル
    let case4 = input(vec![
        file(
            ChangeKind::Modified,
            "資料/第3章_実験.docx",
            "段落 +60 / -0",
            Some(
                (0..60)
                    .map(|i| format!("+ 第{i}節では追加の実験条件を整理した。"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "資料/売上管理.xlsx",
            "セル 40 件",
            Some(
                (0..40)
                    .map(|i| {
                        cell_line(
                            "Sheet1",
                            &format!("B{}", i + 4),
                            Some(&(120_000 + i * 10).to_string()),
                            Some(&(135_000 + i * 10).to_string()),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
    ]);
    // 5: 5 ファイル（ソースと文書の混在、差分合計 7 万文字程度）
    let case5 = input(vec![
        file(
            ChangeKind::Modified,
            "src/app/ui_handler.ts",
            "行 +500 / -10",
            Some(
                (0..500)
                    .map(|i| format!("+ export const handler_{i} = (value: number) => value * {i};"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "src/core/parser.rs",
            "行 +400 / -0",
            Some(
                (0..400)
                    .map(|i| format!("+ pub fn read_{i}(input: &str) -> Result<Vec<u8>, Error> {{ todo!() }}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "資料/第3章_実験.docx",
            "段落 +150 / -3",
            Some(
                (0..150)
                    .map(|i| format!("+ 第{i}項では、実験の条件と結果の対応を表にまとめて示した。"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "議事録/週次会議.md",
            "行 +400 / -0",
            Some(
                (0..400)
                    .map(|i| format!("+ 議題{i}について、担当者が進捗と次回までの作業を報告した。"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        file(
            ChangeKind::Modified,
            "資料/売上管理.xlsx",
            "セル 200 件",
            Some(
                (0..200)
                    .map(|i| {
                        cell_line(
                            "Sheet1",
                            &format!("C{}", i + 2),
                            Some(&format!("{}", 1000 + i)),
                            Some(&format!("{}", 2000 + i)),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
    ]);
    // 6: 35 ファイル（長い日本語名。各ファイルに短い差分）
    let case6 = input(
        (0..35)
            .map(|i| {
                let kind = if i % 7 == 0 {
                    ChangeKind::Added
                } else {
                    ChangeKind::Modified
                };
                file(
                    kind,
                    &format!("2024年度_第{i:02}回_プロジェクト進捗報告書_最終確認版_修正{i}.docx"),
                    &format!("段落 +{} / -{}", i % 4 + 1, i % 2),
                    Some(format!("+ 第{i}回の進捗と次回までの作業内容を追記した。")),
                )
            })
            .collect(),
    );
    vec![
        ("1 ソース 1 ファイル 500 行", case1),
        ("2 ソース 3 ファイル約 300 行", case2),
        ("3 日本語 md 800 行", case3),
        ("4 docx 60 段落＋xlsx 40 セル", case4),
        ("5 5 ファイル（混在・約 7 万文字）", case5),
        ("6 35 ファイル（長い日本語名）", case6),
    ]
}

/// 環境変数からサーバーの接続情報を読む。未設定なら `None`（テストは省略する）
fn server_env() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::var_os("HIKAE_TEST_LLAMA_SERVER")?;
    let model = std::env::var_os("HIKAE_TEST_MODEL")?;
    Some((PathBuf::from(exe), PathBuf::from(model)))
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要（環境変数 HIKAE_TEST_LLAMA_SERVER、HIKAE_TEST_MODEL）"]
async fn real_llama_server_returns_valid_sentence() {
    let Some((exe, model)) = server_env() else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    assert!(model.is_file(), "モデルファイルがありません");

    let server = LlamaServer::new(LlamaServerOptions::new(Some(exe))).unwrap();
    let base = server.ensure_started(&model).await.unwrap();
    let chat = ChatClient::new().unwrap();

    // 全件の結果を出してから判定する（途中で止めると、残りの出力を確認できないため）
    let mut rejected = Vec::new();
    for (title, input) in sample_inputs() {
        let messages = build_messages(&input);
        let raw = chat
            .chat(&base, "model", &messages, GENERATE_TIMEOUT)
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

/// 1 回の生成の結果（計測用）
struct Run {
    /// 検証を通ったか（救済を含む）
    passed: bool,
    /// 救済を使わずに通ったか（救済の導入前の検証と同じ条件）
    strict: bool,
    prompt_tokens: Option<u64>,
    elapsed: Duration,
    /// 通信・サーバーのエラー（あれば）
    failed_call: bool,
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要。計測用（HIKAE_TEST_RUNS 回ずつ生成し、表を出す）"]
async fn measure_suggestion_inputs() {
    let Some((exe, model)) = server_env() else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    let runs: usize = std::env::var("HIKAE_TEST_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let only = std::env::var("HIKAE_TEST_ONLY").ok();

    let mut options = LlamaServerOptions::new(Some(exe));
    if let Some(threads) = std::env::var("HIKAE_TEST_THREADS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        options.threads = threads;
    }
    let threads = options.threads;
    let server = LlamaServer::new(options).unwrap();
    let base = server.ensure_started(&model).await.unwrap();
    let chat = ChatClient::new().unwrap();

    println!("計測: threads={threads}, runs={runs}");
    println!("| 入力 | 通過（救済を含む） | 通過（救済なし） | 通信エラー | 入力トークン | 所要 中央値 | 所要 最大 |");
    println!("| --- | --- | --- | --- | --- | --- | --- |");
    for (title, input) in measurement_inputs() {
        if let Some(only) = &only {
            if !title.starts_with(only.as_str()) {
                continue;
            }
        }
        let mut results = Vec::new();
        for i in 0..runs {
            let messages = build_messages(&input);
            let started = Instant::now();
            let outcome = chat
                .chat_with_usage(&base, "model", &messages, GENERATE_TIMEOUT)
                .await;
            let elapsed = started.elapsed();
            let run = match outcome {
                Ok(out) => {
                    let cleaned = clean_summary_detail(&out.content);
                    println!(
                        "[{title}] run {i}: prompt_tokens={:?} elapsed_ms={} raw={}",
                        out.prompt_tokens,
                        elapsed.as_millis(),
                        out.content
                    );
                    Run {
                        passed: cleaned.is_some(),
                        strict: cleaned.as_ref().is_some_and(|c| !c.rescued),
                        prompt_tokens: out.prompt_tokens,
                        elapsed,
                        failed_call: false,
                    }
                }
                Err(e) => {
                    println!(
                        "[{title}] run {i}: error={e:?} elapsed_ms={}",
                        elapsed.as_millis()
                    );
                    Run {
                        passed: false,
                        strict: false,
                        prompt_tokens: None,
                        elapsed,
                        failed_call: true,
                    }
                }
            };
            results.push(run);
        }
        print_row(title, &results);
    }
    server.stop().await;
}

/// 計測の 1 行（表）を出す
fn print_row(title: &str, runs: &[Run]) {
    let n = runs.len();
    let passed = runs.iter().filter(|r| r.passed).count();
    let strict = runs.iter().filter(|r| r.strict).count();
    let errors = runs.iter().filter(|r| r.failed_call).count();
    let mut tokens: Vec<u64> = runs.iter().filter_map(|r| r.prompt_tokens).collect();
    tokens.sort_unstable();
    let token_text = match (tokens.first(), tokens.last()) {
        (Some(min), Some(max)) if min == max => format!("{min}"),
        (Some(min), Some(max)) => format!("{min}〜{max}"),
        _ => "-".to_string(),
    };
    let mut millis: Vec<u128> = runs.iter().map(|r| r.elapsed.as_millis()).collect();
    millis.sort_unstable();
    let median = millis.get(n / 2).copied().unwrap_or(0);
    let max = millis.last().copied().unwrap_or(0);
    println!(
        "| {title} | {passed}/{n} | {strict}/{n} | {errors}/{n} | {token_text} | {:.1} 秒 | {:.1} 秒 |",
        median as f64 / 1000.0,
        max as f64 / 1000.0
    );
}

#[test]
fn measurement_inputs_are_built_without_a_server() {
    // 計測の入力が組み立てられること（サーバーを使わない確認）
    let inputs = measurement_inputs();
    assert_eq!(inputs.len(), 6);
    let total_diff: usize = inputs[4]
        .1
        .files
        .iter()
        .filter_map(|f| f.diff_text.as_ref())
        .map(|t| t.chars().count())
        .sum();
    assert!(
        total_diff > 40_000,
        "入力 5 の差分が小さすぎる: {total_diff}"
    );
    assert_eq!(inputs[5].1.files.len(), 35);
}
