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
// テキストの追記（`measure_text_append`）は、追跡中の既存ファイルへの追記と未追跡の新規ファイルの組み合わせ 9 件で、
// 1 回目の出力が件数だけなら 1 回書き直したあとの内容ヒット 90% 以上を確かめる（評価ノート 13 節）。
// 内容の評価（`measure_content_understanding`）は評価ノート 11.4 の 12 件を使い、検証通過 95% 以上と
// 内容ヒット 90% 以上を確かめる（HIKAE_TEST_RUNS 回ずつ。既定 3）。語の一致による判定であり、事実の正否は判定しない。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    build_messages, cell_line, clean_summary, clean_summary_detail, content_retry_messages,
    needs_content_retry, ChangeInput, ChangeKind, ChangedFile, ChatClient, LlamaServer,
    LlamaServerOptions, GENERATE_TIMEOUT,
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

// ========== 内容の理解の評価（評価ノート 11.4 の 12 件。keyFacts は語の一致で判定する） ==========

/// 内容を問う評価の入力 1 件
struct ContentCase {
    id: &'static str,
    input: ChangeInput,
    /// 内容が入力から分からない変更（削除のみ・画像）。内容ヒットの分母から外す
    no_content: bool,
    /// 出力にいずれか含まれれば「内容に触れた」とみなす語（評価ノート 11.4）
    key_facts: &'static [&'static str],
}

fn content_case(
    id: &'static str,
    no_content: bool,
    key_facts: &'static [&'static str],
    files: Vec<ChangedFile>,
) -> ContentCase {
    ContentCase {
        id,
        input: input(files),
        no_content,
        key_facts,
    }
}

fn renamed(name: &str, old: &str, amount: &str, diff: &str) -> ChangedFile {
    ChangedFile {
        kind: ChangeKind::Renamed,
        name: name.into(),
        old_name: Some(old.into()),
        amount: Some(amount.into()),
        diff_text: Some(diff.into()),
    }
}

/// 評価ノート 11.4 の入力 12 件（評価の `inputs2.mjs` と同じ内容）
fn content_inputs() -> Vec<ContentCase> {
    use ChangeKind::{Added as A, Deleted as D, Modified as M};
    vec![
        content_case(
            "01",
            false,
            &["30度", "30℃", "30°C", "1.8倍", "1.8"],
            vec![file(
                M,
                "第3章_実験.docx",
                "段落 +2 / -0",
                Some("+ 追加実験として、温度を30度に上げた条件でも測定を行った。\n+ その結果、反応速度は常温の約1.8倍となった。".into()),
            )],
        ),
        content_case(
            "02",
            false,
            &[
                "135000", "135,000", "13.5万", "87000", "87,000", "8.7万", "120000", "12万",
                "98000", "9.8万", "846000", "84.6万", "東京支店", "大阪支店", "東京", "大阪",
            ],
            vec![file(
                M,
                "売上管理.xlsx",
                "セル 3 件",
                Some("2024年度!B4(東京支店): 120000 → 135000\n2024年度!B5(大阪支店): 98000 → 87000\n2024年度!B12(合計): 842000 → 846000".into()),
            )],
        ),
        content_case(
            "03",
            false,
            &["90秒", "90", "タイムアウト", "待ち時間", "待機時間", "制限時間"],
            vec![file(
                M,
                "app_settings.ini",
                "行 +1 / -1",
                Some("- timeout_seconds = 30\n+ timeout_seconds = 90".into()),
            )],
        ),
        content_case(
            "04",
            false,
            &[
                "最後", "最終", "ずれ", "抜け", "含め", "+ 1", "+1", "total + 1", "範囲", "1 から",
                "1から", "ページ番号",
            ],
            vec![file(
                M,
                "pagination.py",
                "行 +1 / -1",
                Some("@@ def page_numbers(total):\n-     return list(range(1, total))\n+     return list(range(1, total + 1))".into()),
            )],
        ),
        content_case(
            "05",
            false,
            &["復元", "戻す", "戻し", "履歴タブ", "履歴", "元に戻"],
            vec![file(
                M,
                "README.md",
                "行 +5 / -0",
                Some("+ ## 復元のしかた\n+ 1. 「履歴」タブを開く\n+ 2. 戻したい日付の保存を選ぶ\n+ 3. 「この時点に戻す」を押す\n+ 戻す前の状態も自動で残ります。".into()),
            )],
        ),
        content_case(
            "06",
            false,
            &["木曜", "14時", "14 時", "佐藤", "火曜", "田中", "定例", "担当"],
            vec![file(
                M,
                "議事録_2024-06-12.md",
                "行 +2 / -2",
                Some("- 次回の定例は毎週火曜 10 時に開催する。\n- 資料の取りまとめは田中が担当する。\n+ 次回の定例は毎週木曜 14 時に開催する。\n+ 資料の取りまとめは佐藤が担当する。".into()),
            )],
        ),
        content_case(
            "07",
            false,
            &["8%", "10%", "8％", "10％", "税率", "消費税", "税込", "1.08", "1.1"],
            vec![
                file(M, "見積書.xlsx", "セル 4 件", Some("Sheet1!D12(税率): 8% → 10%\nSheet1!E12(消費税): 24000 → 30000\nSheet1!E13(合計): 324000 → 330000\nSheet1!B2(備考): 消費税 8% を含みます → 消費税 10% を含みます".into())),
                file(M, "請求書テンプレート.docx", "段落 +2 / -2", Some("- 消費税（8%）は別途加算されます。\n- 税込価格は税抜価格の 1.08 倍です。\n+ 消費税（10%）は別途加算されます。\n+ 税込価格は税抜価格の 1.10 倍です。".into())),
                file(M, "料金表.md", "行 +3 / -3", Some("- | 標準プラン | 10,000円 | 10,800円 |\n- | 上位プラン | 20,000円 | 21,600円 |\n- | 税率 | 8% | |\n+ | 標準プラン | 10,000円 | 11,000円 |\n+ | 上位プラン | 20,000円 | 22,000円 |\n+ | 税率 | 10% | |".into())),
                file(M, "契約書ひな形.docx", "段落 +1 / -1", Some("- 報酬には消費税 8% を含まない。\n+ 報酬には消費税 10% を含まない。".into())),
                file(M, "お知らせ_価格改定.docx", "段落 +1 / -1", Some("- 当社の価格は消費税 8% で計算しております。\n+ 当社の価格は消費税 10% で計算しております。".into())),
                file(M, "計算シート.xlsx", "セル 2 件", Some("Sheet1!B1(税率): 0.08 → 0.1\nSheet1!B2(税込): =A2*1.08 → =A2*1.1".into())),
            ],
        ),
        content_case(
            "08",
            false,
            &[
                "横浜", "神奈川", "住所", "会議室", "予約", "旅費", "交通費", "宿泊費", "14480",
                "14,480", "4680", "4,680", "9800", "9,800", "targetSdkVersion", "34", "年間行事",
                "行事予定",
            ],
            vec![
                file(M, "履歴書.docx", "段落 +1 / -1", Some("- 住所: 東京都新宿区西新宿 2-8-1\n+ 住所: 神奈川県横浜市西区みなとみらい 1-1-1".into())),
                file(M, "会議室予約.md", "行 +2 / -0", Some("+ - 5/20 14:00-15:00 第2会議室: 営業会議\n+ - 5/21 10:00-11:30 第1会議室: 新人研修".into())),
                file(M, "旅費精算.xlsx", "セル 3 件", Some("Sheet1!C5(交通費): 3200 → 4680\nSheet1!C6(宿泊費): 0 → 9800\nSheet1!C10(合計): 3200 → 14480".into())),
                file(M, "build.gradle", "行 +1 / -1", Some("- targetSdkVersion 33\n+ targetSdkVersion 34".into())),
                file(A, "年間行事予定.md", "行 +12", Some("+ # 2024年度 年間行事予定\n+ - 4月 入社式\n+ - 7月 夏季休業\n+ - 10月 創立記念日\n+ - 12月 忘年会\n+ - 3月 決算".into())),
            ],
        ),
        content_case(
            "09",
            false,
            &["出店", "2店舗", "2 店舗", "15枚", "15 枚", "横浜", "大宮", "結論"],
            vec![renamed(
                "発表資料_最終版.md",
                "発表資料_仮.md",
                "行 +3 / -2",
                "- スライド枚数: 12 枚\n- 結論: 来期は現状の 1 店舗を維持する。\n+ スライド枚数: 15 枚\n+ 結論: 来期は新規に 2 店舗を出店する。\n+ 出店候補は横浜と大宮とする。",
            )],
        ),
        content_case(
            "10",
            true,
            &[],
            vec![
                file(D, "会議メモ_2023.docx", "削除", None),
                file(D, "見積_旧.xlsx", "削除", None),
            ],
        ),
        content_case(
            "11",
            true,
            &[],
            vec![file(
                M,
                "図4_結果グラフ.png",
                "画像（1200x800 → 1200x800、ファイルサイズ 84KB → 112KB）",
                None,
            )],
        ),
        content_case(
            "12",
            false,
            &["考察", "考查", "誤字", "誤り", "誤記", "漢字", "表記"],
            vec![file(
                M,
                "報告書.docx",
                "段落 +1 / -1",
                Some("- 実験結果についての考查を以下に示す。\n+ 実験結果についての考察を以下に示す。".into()),
            )],
        ),
    ]
}

#[test]
fn content_inputs_match_the_evaluation_set() {
    // 評価ノート 11.4 の構成: 12 件、そのうち内容を問えないもの（削除のみ・画像）は 2 件
    let cases = content_inputs();
    assert_eq!(cases.len(), 12);
    assert_eq!(cases.iter().filter(|c| c.no_content).count(), 2);
    for case in &cases {
        if case.no_content {
            assert!(case.key_facts.is_empty(), "{}", case.id);
        } else {
            assert!(!case.key_facts.is_empty(), "{}", case.id);
        }
    }
    assert_eq!(cases[6].input.files.len(), 6, "入力 07 は 6 ファイル");
    assert_eq!(cases[7].input.files.len(), 5, "入力 08 は 5 ファイル");
}

/// 内容ヒット（検証を通った文に、いずれかの語が含まれる）
fn content_hit(text: &str, key_facts: &[&str]) -> bool {
    key_facts.iter().any(|k| text.contains(k))
}

/// 評価の V0（現行の文面。V1 採用の前）。比較の計測だけに使う（本体のプロンプトは変えない）
const V0_SYSTEM: &str = "あなたは、ファイルの変更内容から「保存メモ」を書く assistant です。
規則:
- 出力は日本語で 1 文のみ。40 文字程度まで。
- 複数のファイルを変更したときも、全体を 1 文にまとめる。ファイルごとに分けて書かない。
- 変更ファイルが 3 件を超えるときは、ファイル名を並べず、種類と件数でまとめる（例: 文書 35 件を更新し、5 件を追加した）。
- 与えられた情報にある事実だけを書く。推測や評価は書かない。
- 専門用語（commit、diff、push、branch、repository など）は使わない。
- ファイル名や、変更前後の具体的な値が分かる場合は、それを使う。
- 前置き、説明、引用符、記号の装飾は付けない。メモの文だけを出力する。";

/// 評価の V0 のユーザーメッセージの末尾
const V0_TAIL: &str = "複数のファイルがあっても、全体を 1 文にまとめてください。ファイルが多いときは、ファイル名を並べず件数でまとめてください。";

/// 現行（V1）の末尾の指示（`prompt.rs` の `USER_TAIL` と同じ文面。V0 への差し替えに使う）
const V1_TAIL: &str = "何のファイルかではなく、何がどう変わったか（値・内容・目的）を 1 文で書いてください。ファイル名は必要なときだけ添えてください。";

/// 計測に使うメッセージ。`HIKAE_TEST_PROMPT=v0` のときは評価の V0 の文面に差し替える（比較用）
fn measurement_messages(input: &ChangeInput, v0: bool) -> Vec<core_llm::Message> {
    let mut messages = build_messages(input);
    if v0 {
        messages[0].content = V0_SYSTEM.to_string();
        let user = messages[1].content.trim_end();
        let body = user.strip_suffix(V1_TAIL).unwrap();
        messages[1].content = format!("{body}{V0_TAIL}\n");
    }
    messages
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要。内容の評価（HIKAE_TEST_RUNS 回ずつ。既定 3）"]
async fn measure_content_understanding() {
    let Some((exe, model)) = server_env() else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    let runs: usize = std::env::var("HIKAE_TEST_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
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

    // HIKAE_TEST_PROMPT=v0 のとき、比較用に評価の V0 の文面で計測する（既定は現行の文面）
    let v0 = std::env::var("HIKAE_TEST_PROMPT").is_ok_and(|v| v == "v0");
    let variant = if v0 { "v0" } else { "現行" };
    println!("内容の評価: threads={threads}, runs={runs}, prompt={variant}");
    println!("| 入力 | 検証通過 | 内容ヒット | 所要 中央値 | 所要 最大 |");
    println!("| --- | --- | --- | --- | --- |");
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut content_total = 0usize;
    let mut content_hits = 0usize;
    let mut millis_all: Vec<u128> = Vec::new();
    for case in content_inputs() {
        let mut case_passed = 0usize;
        let mut case_hits = 0usize;
        let mut millis: Vec<u128> = Vec::new();
        for i in 0..runs {
            let messages = measurement_messages(&case.input, v0);
            let started = Instant::now();
            let outcome = chat
                .chat_with_usage(&base, "model", &messages, GENERATE_TIMEOUT)
                .await;
            let elapsed = started.elapsed();
            millis.push(elapsed.as_millis());
            total += 1;
            let cleaned = match outcome {
                Ok(out) => {
                    println!(
                        "[{}] run {i}: elapsed_ms={} raw={}",
                        case.id,
                        elapsed.as_millis(),
                        out.content
                    );
                    clean_summary_detail(&out.content)
                }
                Err(e) => {
                    println!("[{}] run {i}: error={e:?}", case.id);
                    None
                }
            };
            if let Some(c) = cleaned {
                passed += 1;
                case_passed += 1;
                let hit = !case.no_content && content_hit(&c.text, case.key_facts);
                if hit {
                    content_hits += 1;
                    case_hits += 1;
                }
                println!("[{}] run {i}: text={:?} hit={hit}", case.id, c.text);
            }
        }
        if !case.no_content {
            content_total += runs;
        }
        millis.sort_unstable();
        millis_all.extend(millis.iter().copied());
        let median = millis.get(runs / 2).copied().unwrap_or(0);
        let max = millis.last().copied().unwrap_or(0);
        let hit_text = if case.no_content {
            "（内容なし）".to_string()
        } else {
            format!("{case_hits}/{runs}")
        };
        println!(
            "| {} | {case_passed}/{runs} | {hit_text} | {:.1} 秒 | {:.1} 秒 |",
            case.id,
            median as f64 / 1000.0,
            max as f64 / 1000.0
        );
    }
    server.stop().await;
    assert!(!server.is_running().await);

    millis_all.sort_unstable();
    let median_all = millis_all.get(millis_all.len() / 2).copied().unwrap_or(0);
    let max_all = millis_all.last().copied().unwrap_or(0);
    let pct = |a: usize, b: usize| {
        if b == 0 {
            0.0
        } else {
            a as f64 * 100.0 / b as f64
        }
    };
    println!(
        "検証通過 {passed}/{total} ({:.1}%)、内容ヒット {content_hits}/{content_total} ({:.1}%)、所要 中央値 {:.1} 秒・最大 {:.1} 秒",
        pct(passed, total),
        pct(content_hits, content_total),
        median_all as f64 / 1000.0,
        max_all as f64 / 1000.0
    );
    // 目標（評価ノート 11 の V1 の採用条件）: 内容ヒット 90% 以上、検証通過 95% 以上
    assert!(
        pct(content_hits, content_total) >= 90.0,
        "内容ヒットが 90% 未満: {content_hits}/{content_total}"
    );
    assert!(
        pct(passed, total) >= 95.0,
        "検証通過が 95% 未満: {passed}/{total}"
    );
}

/// テキストの追記の計測に使う入力（合成データ）。追跡中の `test.txt` に `tracked` を、未追跡の新規ファイルに `new_file` を追記した形。
struct AppendCase {
    id: &'static str,
    tracked: &'static [&'static str],
    new_file: &'static [&'static str],
    /// 内容に触れたとみなす語（いずれかが出力に含まれれば内容ヒット）
    keys: &'static [&'static str],
}

/// 追記の入力 9 件。(a) 意味のある文、(b) 短く意味の薄い文、(c) 英数字、(d)〜(f) 形式の違い、(g)(h) チェックリストとコード風、(i) 多行（件数だけの文が出やすい）
const APPEND_CASES: &[AppendCase] = &[
    AppendCase {
        id: "a",
        tracked: &["明日の会議は10時から。", "資料は3部印刷する。"],
        new_file: &["牛乳と卵を買う"],
        keys: &["会議", "10時", "資料", "3部", "印刷", "牛乳", "卵", "買う"],
    },
    AppendCase {
        id: "b",
        tracked: &["テスト", "aaa"],
        new_file: &["あいうえお"],
        keys: &["テスト", "aaa", "あいうえお"],
    },
    AppendCase {
        id: "c",
        tracked: &["Hello world 2026", "12345"],
        new_file: &["ABC-123"],
        keys: &["Hello", "world", "2026", "12345", "ABC", "123"],
    },
    AppendCase {
        id: "d",
        tracked: &["12:00に集合", "会場はA館"],
        new_file: &["メモ1"],
        keys: &["集合", "12:00", "A館", "メモ"],
    },
    AppendCase {
        id: "e",
        tracked: &["ok", "done"],
        new_file: &["x"],
        keys: &["ok", "done", "x"],
    },
    AppendCase {
        id: "f",
        tracked: &["税率を8%から10%に変更"],
        new_file: &["追記: 2024年度版"],
        keys: &["税率", "10%", "2024年度", "8%"],
    },
    AppendCase {
        id: "g",
        tracked: &["- [ ] 牛乳", "- [ ] 卵"],
        new_file: &["2024/10/12 提出"],
        keys: &["牛乳", "卵", "提出", "2024"],
    },
    AppendCase {
        id: "h",
        tracked: &["x = 1", "y = 2"],
        new_file: &["print(x)"],
        keys: &["x =", "y =", "print", "x", "y"],
    },
    AppendCase {
        id: "i",
        tracked: &[
            "項目A: 準備",
            "項目B: 実施",
            "項目C: 報告",
            "項目D: 次回の予定",
            "項目E: 費用の精算",
        ],
        new_file: &["付録1", "付録2", "付録3"],
        keys: &["項目", "付録", "準備", "実施", "報告", "精算"],
    },
];

/// 追記の入力を、アプリの組み立て（`collect_input` と同じ形: 種別・名前・変更量・本文）で作る
fn append_input(case: &AppendCase) -> ChangeInput {
    let body = |lines: &[&str]| lines.iter().map(|l| format!("+ {l}\n")).collect::<String>();
    input(vec![
        ChangedFile {
            kind: ChangeKind::Modified,
            name: "test.txt".into(),
            old_name: None,
            amount: Some(format!("行 +{} / -0", case.tracked.len())),
            diff_text: Some(body(case.tracked)),
        },
        ChangedFile {
            kind: ChangeKind::Added,
            name: "新規 テキスト ドキュメント.txt".into(),
            old_name: None,
            amount: Some(format!("行 +{} / -0", case.new_file.len())),
            diff_text: Some(body(case.new_file)),
        },
    ])
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要。テキストの追記の計測（HIKAE_TEST_RUNS 回ずつ。既定 5）"]
async fn measure_text_append() {
    let Some((exe, model)) = server_env() else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    let runs: usize = std::env::var("HIKAE_TEST_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let server = LlamaServer::new(LlamaServerOptions::new(Some(exe))).unwrap();
    let base = server.ensure_started(&model).await.unwrap();
    let chat = ChatClient::new().unwrap();

    // 1 回目の出力と、service と同じ書き直し（件数だけで内容に触れていないときに 1 回）の後の出力を計る
    println!("| 入力 | 1 回目の内容ヒット | 書き直し | 書き直し後の内容ヒット |");
    println!("| --- | --- | --- | --- |");
    let (mut total, mut first_hits, mut final_hits) = (0usize, 0usize, 0usize);
    for case in APPEND_CASES {
        let input = append_input(case);
        let messages = build_messages(&input);
        let (mut case_first, mut case_final, mut retried) = (0usize, 0usize, 0usize);
        for i in 0..runs {
            let raw = chat
                .chat(&base, "model", &messages, GENERATE_TIMEOUT)
                .await
                .unwrap();
            let first = clean_summary(&raw);
            let mut result = first.clone();
            if let Some(text) = first.as_deref() {
                if needs_content_retry(&input, text) {
                    retried += 1;
                    let raw2 = chat
                        .chat(
                            &base,
                            "model",
                            &content_retry_messages(&input),
                            GENERATE_TIMEOUT,
                        )
                        .await
                        .unwrap();
                    if let Some(text2) = clean_summary(&raw2) {
                        if !needs_content_retry(&input, &text2) {
                            result = Some(text2);
                        }
                    }
                }
            }
            total += 1;
            let hit = |t: &Option<String>| {
                t.as_deref()
                    .is_some_and(|t| case.keys.iter().any(|k| t.contains(k)))
            };
            if hit(&first) {
                case_first += 1;
                first_hits += 1;
            }
            if hit(&result) {
                case_final += 1;
                final_hits += 1;
            }
            println!("[{}] run {i}: first={first:?} result={result:?}", case.id);
        }
        println!(
            "| {} | {case_first}/{runs} | {retried}/{runs} | {case_final}/{runs} |",
            case.id
        );
    }
    server.stop().await;
    assert!(!server.is_running().await);

    println!(
        "1 回目の内容ヒット {first_hits}/{total}、書き直し後の内容ヒット {final_hits}/{total}"
    );
    // 目標: 書き直し後の内容ヒット 90% 以上（件数だけの文が残らないこと）
    assert!(
        final_hits * 100 >= total * 90,
        "書き直し後の内容ヒットが 90% 未満: {final_hits}/{total}"
    );
}
