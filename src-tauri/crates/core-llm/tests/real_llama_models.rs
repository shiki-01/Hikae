// 現実的な入力でのモデル比較（評価ノート 15 節）。実機の llama-server と GGUF を使う。通常の `cargo test` では実行しない（#[ignore]）。
//
// 実行例（環境変数は実行前に設定する）:
//   HIKAE_TEST_LLAMA_SERVER=<llama-server の実行ファイル> HIKAE_TEST_MODEL=<GGUF のパス>
//   HIKAE_TEST_MODEL_LABEL=<表示用の名前> HIKAE_TEST_RUNS=<入力ごとの回数（既定 1）> HIKAE_TEST_OUT=<結果の JSON の出力先>
//   cargo test -p core-llm --test real_llama_models -- --ignored --nocapture
//
// 入力は合成データ 32 件（A〜F の 6 カテゴリ）。差分は `prepare_input` に通す（アプリの保存メモの入力と同じ組み立て）。
// 生成は `LlmService::suggest_custom` と同じ順序で行う（`service.rs` の `suggest_with` と同じ関数の並び）。
// 違いは、書き直しの有無を記録するために、サービスの内部関数を公開関数で組み直している点だけ。
// 書き直しは `needs_content_retry` が真のときに、残りの時間（上限 `GENERATE_TIMEOUT`）の中で 1 回だけ行う。
// キャッシュ（`cache_prompt`）は既定のまま（アプリと同じ要求）。
//
// 指標は語の一致による判定（内容ヒット）と、機械的な定型の判定。優劣の判断は含めない。
// 評価ノートの表は、JSON の内容を別に整形して作る（このテストは表を作らない）。

// 統合テストは全体がテストコードのため unwrap を許可する（CLAUDE.md の規約はテスト以外での禁止）
#![allow(clippy::unwrap_used)]

use core_llm::{
    build_messages, cell_amount, cell_line, content_retry_messages, copies_diff, failure_of,
    looks_sensitive, needs_content_retry, paragraph_amount, parse_memo, prepare_input, screen_memo,
    signed_line, text_amount, ChangeInput, ChangeKind, ChatClient, ChatOutput, LlamaServer,
    LlamaServerOptions, LlmError, Memo, Message, SourceChange, SuggestNotice, GENERATE_TIMEOUT,
};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 生成の要求の model 名（`service.rs` の `CUSTOM_MODEL_NAME` と同じ値。内蔵のサーバーは既定の 1 モデルだけを読む）
const MODEL_NAME: &str = "custom-gguf";

/// 定型の語（件数・操作の語・助詞）。要約がこれだけで書けていれば「定型のみ」とみなす
const TEMPLATE_WORDS: &[&str] = &[
    "内容が",
    "されました",
    "しました",
    "された",
    "した",
    "する",
    "ファイル",
    "更新",
    "追加",
    "削除",
    "変更",
    "修正",
    "移動",
    "名前",
    "件数",
    "件",
    "行",
    "段落",
    "セル",
    "シート",
    "箇所",
    "個",
    "枚",
    "の",
    "を",
    "に",
    "が",
    "は",
    "と",
    "で",
    "も",
    "や",
    "し",
    "た",
    "て",
    "、",
    "。",
];

/// 評価の 1 入力（合成データ）
struct Case {
    id: &'static str,
    category: &'static str,
    /// 人間が書く「良いメモ」の参考文（評価者が比べるため。モデルへは渡さない）
    reference: &'static str,
    /// 正解の要点の語（1 つでも要約か本文に含まれれば内容ヒット）。空は内容を問わない入力
    key_facts: &'static [&'static str],
    changes: Vec<SourceChange>,
}

/// テキスト差分（`+ 行` / `- 行` の形）。削除された行を先に並べる
fn diff_lines(added: &[&str], removed: &[&str]) -> String {
    removed
        .iter()
        .map(|line| signed_line(false, line))
        .chain(added.iter().map(|line| signed_line(true, line)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// テキストファイルの変更（追記・削除の行）
fn text_modified(path: &str, added: &[&str], removed: &[&str]) -> SourceChange {
    SourceChange {
        kind: ChangeKind::Modified,
        path: path.into(),
        old_path: None,
        amount: Some(text_amount(added.len(), removed.len())),
        diff_text: Some(diff_lines(added, removed)),
    }
}

/// 新規のテキストファイル
fn text_added(path: &str, added: &[&str]) -> SourceChange {
    SourceChange {
        kind: ChangeKind::Added,
        path: path.into(),
        old_path: None,
        amount: Some(text_amount(added.len(), 0)),
        diff_text: Some(diff_lines(added, &[])),
    }
}

/// docx の段落の変更（抽出したテキストの差分）
fn paragraph_modified(path: &str, added: &[&str], removed: &[&str]) -> SourceChange {
    SourceChange {
        kind: ChangeKind::Modified,
        path: path.into(),
        old_path: None,
        amount: Some(paragraph_amount(added.len(), removed.len())),
        diff_text: Some(diff_lines(added, removed)),
    }
}

/// xlsx のセルの変更（`cell_line` の形。シートごとに 1 行）
fn xlsx_change(path: &str, cells: &[String]) -> SourceChange {
    SourceChange {
        kind: ChangeKind::Modified,
        path: path.into(),
        old_path: None,
        amount: Some(cell_amount(cells.len())),
        diff_text: Some(cells.join("\n")),
    }
}

/// 差分の本文が無いファイル（削除・画像・名前変更など）
fn bare(kind: ChangeKind, path: &str) -> SourceChange {
    SourceChange {
        kind,
        path: path.into(),
        old_path: None,
        amount: None,
        diff_text: None,
    }
}

/// 名前変更（フォルダの移動を含む）
fn renamed(old: &str, new: &str) -> SourceChange {
    SourceChange {
        kind: ChangeKind::Renamed,
        path: new.into(),
        old_path: Some(old.into()),
        amount: None,
        diff_text: None,
    }
}

/// セルの 1 行（`cell_line` の薄いラッパー）
fn cell(sheet: &str, at: &str, before: Option<&str>, after: Option<&str>) -> String {
    cell_line(sheet, at, before, after)
}

/// 32 件の入力。カテゴリは A（短い・意味の薄いテキスト）8、B（docx）6、C（xlsx）5、D（コード・設定）6、E（複数ファイル）4、F（構造）3。
fn cases() -> Vec<Case> {
    vec![
        // A: 短い・意味の薄いテキスト編集
        Case {
            id: "A01",
            category: "A",
            reference: "test.txt に「今日はいい天気ですね」「へへ」を追記し、新規の テキスト ドキュメントに「TODO:明日はここを進める」と書いた。",
            key_facts: &["天気", "へへ", "TODO", "明日"],
            changes: vec![
                text_modified("test.txt", &["今日はいい天気ですね", "へへ"], &[]),
                text_added("新規 テキスト ドキュメント.txt", &["TODO:明日はここを進める"]),
            ],
        },
        Case {
            id: "A02",
            category: "A",
            reference: "日記に、雨の日は家で映画を見たという一文を追記した。",
            key_facts: &["雨", "映画"],
            changes: vec![text_modified(
                "日記/2026-10.txt",
                &["今日は雨だったので家で映画を見た。"],
                &[],
            )],
        },
        Case {
            id: "A03",
            category: "A",
            reference: "todo.txt に「見積書のひな形を確認する」「取引先に電話で日程を確認」の 2 項目を追加した。",
            key_facts: &["見積書", "ひな形", "取引先", "日程"],
            changes: vec![text_modified(
                "todo.txt",
                &["- [ ] 見積書のひな形を確認する", "- [ ] 取引先に電話で日程を確認"],
                &[],
            )],
        },
        Case {
            id: "A04",
            category: "A",
            reference: "担当メモの担当者を、田中から佐藤に変えた。",
            key_facts: &["佐藤"],
            changes: vec![text_modified("担当メモ.txt", &["担当は佐藤"], &["担当は田中"])],
        },
        Case {
            id: "A05",
            category: "A",
            reference: "notes.txt に空行を 2 行追加した（文字の内容は変わっていない）。",
            key_facts: &[],
            changes: vec![text_modified("notes.txt", &["", ""], &[])],
        },
        Case {
            id: "A06",
            category: "A",
            reference: "メモ1にりんご、メモ2にみかんを追加し、メモ3の「ばなな」を「ぶどう」に変えた。",
            key_facts: &["りんご", "みかん", "ぶどう"],
            changes: vec![
                text_modified("メモ1.txt", &["りんご"], &[]),
                text_modified("メモ2.txt", &["みかん"], &[]),
                text_modified("メモ3.txt", &["ぶどう"], &["ばなな"]),
            ],
        },
        Case {
            id: "A07",
            category: "A",
            reference: "sandbox.txt に「あああ」と「test」を追記した。",
            key_facts: &["あああ", "test"],
            changes: vec![text_modified("sandbox.txt", &["あああ", "test"], &[])],
        },
        Case {
            id: "A08",
            category: "A",
            reference: "新しく買い物.txt を作り、「牛乳と卵を買う」と書いた。",
            key_facts: &["牛乳", "卵"],
            changes: vec![text_added("買い物.txt", &["牛乳と卵を買う"])],
        },
        // B: 文書（docx の段落）
        Case {
            id: "B01",
            category: "B",
            reference: "定例の議事録に、次回会議を11月第2週に開く予定と、議題3の予算案を保留にしたことを追記した。",
            key_facts: &["次回会議", "11月", "予算案", "保留"],
            changes: vec![paragraph_modified(
                "議事録/2026-10-定例.docx",
                &["次回会議は11月第2週に開催する。", "議題3の予算案は保留とした。"],
                &[],
            )],
        },
        Case {
            id: "B02",
            category: "B",
            reference: "売上報告の前年比を「横ばい」から「4%増加」に書き換えた。",
            key_facts: &["4%", "増加"],
            changes: vec![paragraph_modified(
                "報告書/売上報告.docx",
                &["売上は前年比で4%増加した。"],
                &["売上は前年比で横ばいだった。"],
            )],
        },
        Case {
            id: "B03",
            category: "B",
            reference: "新製品案から、旧案の「予算は500万円とする」の段落を削除した。",
            key_facts: &["500万円", "予算"],
            changes: vec![paragraph_modified(
                "企画/新製品案.docx",
                &[],
                &["旧案: 予算は500万円とする。"],
            )],
        },
        Case {
            id: "B04",
            category: "B",
            reference: "経費精算マニュアルに、承認者（部長）と承認後の経理への連絡を追記した。",
            key_facts: &["承認", "経理"],
            changes: vec![paragraph_modified(
                "マニュアル/経費精算.docx",
                &["承認は部長が行う。", "承認後、経理に連絡する。"],
                &[],
            )],
        },
        Case {
            id: "B05",
            category: "B",
            reference: "本社の所在地の記載を、東京から大阪に直した。",
            key_facts: &["大阪"],
            changes: vec![paragraph_modified(
                "報告書/本社案内_最終.docx",
                &["本社は大阪にある。"],
                &["本社は東京にある。"],
            )],
        },
        Case {
            id: "B06",
            category: "B",
            reference: "見積条件の有効期限を、発行から30日から45日に変えた。",
            key_facts: &["45日"],
            changes: vec![paragraph_modified(
                "見積/見積条件.docx",
                &["有効期限は発行から45日とする。"],
                &["有効期限は発行から30日とする。"],
            )],
        },
        // C: 表（xlsx のセル）
        Case {
            id: "C01",
            category: "C",
            reference: "売上管理の4月売上で、C3 を 120万から 135万円に、C4 を 88万から 87万円に直した。",
            key_facts: &["1350000", "870000"],
            changes: vec![xlsx_change(
                "売上/売上管理.xlsx",
                &[
                    cell("4月売上", "C3", Some("1200000"), Some("1350000")),
                    cell("4月売上", "C4", Some("880000"), Some("870000")),
                ],
            )],
        },
        Case {
            id: "C02",
            category: "C",
            reference: "棚卸の商品Aの数量を 120 から 85 に、商品Cの数量を 32 から 0 に減らした。",
            key_facts: &["85"],
            changes: vec![xlsx_change(
                "在庫表.xlsx",
                &[
                    cell("棚卸", "B5", Some("120"), Some("85")),
                    cell("棚卸", "B9", Some("32"), Some("0")),
                ],
            )],
        },
        Case {
            id: "C03",
            category: "C",
            reference: "家計簿に 2026年11月のシートを追加し、B2 に 60000、B3 に 45000 を入れた。",
            key_facts: &["60000", "45000"],
            changes: vec![xlsx_change(
                "家計簿/家計簿.xlsx",
                &[
                    cell("2026年11月", "B2", None, Some("60000")),
                    cell("2026年11月", "B3", None, Some("45000")),
                ],
            )],
        },
        Case {
            id: "C04",
            category: "C",
            reference: "売上管理の5月のシートで、C2 から C9 の 8 件の金額を、それぞれ 5000 円ずつ増やした。",
            key_facts: &["5月", "5000"],
            changes: vec![xlsx_change(
                "売上/売上管理.xlsx",
                &(2..=9)
                    .map(|r| {
                        let at = format!("C{r}");
                        let before = format!("{}", 100_000 + r * 1000);
                        let after = format!("{}", 105_000 + r * 1000);
                        cell("5月", &at, Some(before.as_str()), Some(after.as_str()))
                    })
                    .collect::<Vec<_>>(),
            )],
        },
        Case {
            id: "C05",
            category: "C",
            reference: "点検表のA班B2を OK から NG に、B班C4を未から済に変えた。",
            key_facts: &["NG", "B班"],
            changes: vec![xlsx_change(
                "点検/点検表.xlsx",
                &[
                    cell("A班", "B2", Some("OK"), Some("NG")),
                    cell("B班", "C4", Some("未"), Some("済")),
                ],
            )],
        },
        // D: コード・設定
        Case {
            id: "D01",
            category: "D",
            reference: "税込み価格の計算で、係数を 110/100 から 108/100 に変えた。",
            key_facts: &["108"],
            changes: vec![text_modified(
                "src/tax.rs",
                &["    price * 108 / 100"],
                &["    price * 110 / 100"],
            )],
        },
        Case {
            id: "D02",
            category: "D",
            reference: "設定の通信のタイムアウトを 30 秒から 60 秒に延ばした。",
            key_facts: &["60"],
            changes: vec![text_modified(
                "config/app.toml",
                &["timeout_seconds = 60"],
                &["timeout_seconds = 30"],
            )],
        },
        Case {
            id: "D03",
            category: "D",
            reference: "README に、必要な環境（Python 3.12 以上）の節を追記した。",
            key_facts: &["Python", "3.12"],
            changes: vec![text_modified(
                "README.md",
                &["", "## 必要な環境", "Python 3.12 以上が必要です。"],
                &[],
            )],
        },
        Case {
            id: "D04",
            category: "D",
            reference: "除外設定に、ログファイル（*.log）と dist フォルダを追加した。",
            key_facts: &["ログ", "*.log", "dist"],
            changes: vec![text_modified(".gitignore", &["*.log", "dist/"], &[])],
        },
        Case {
            id: "D05",
            category: "D",
            reference: "package.json のバージョンを 1.4.2 から 1.4.3 に上げた。",
            key_facts: &["1.4.3"],
            changes: vec![text_modified(
                "package.json",
                &["  \"version\": \"1.4.3\","],
                &["  \"version\": \"1.4.2\","],
            )],
        },
        Case {
            id: "D06",
            category: "D",
            reference: "テーマのブランド色を #1a73e8 から #0b5cad に変えた。",
            key_facts: &["#0b5cad", "0b5cad"],
            changes: vec![text_modified(
                "styles/theme.css",
                &["  --brand-color: #0b5cad;"],
                &["  --brand-color: #1a73e8;"],
            )],
        },
        // E: 複数ファイル
        Case {
            id: "E01",
            category: "E",
            reference: "消費税率を8%から10%に変更した（価格表、請求書テンプレート、価格計算のコード）。",
            key_facts: &["10%", "税率", "消費税"],
            changes: vec![
                text_modified("src/price.ts", &["const TAX_RATE = 0.10;"], &["const TAX_RATE = 0.08;"]),
                paragraph_modified("docs/価格表.docx", &["消費税は10%とする。"], &["消費税は8%とする。"]),
                xlsx_change(
                    "請求/請求書テンプレート.xlsx",
                    &[cell("Sheet1", "F20", Some("0.08"), Some("0.1"))],
                ),
            ],
        },
        Case {
            id: "E02",
            category: "E",
            reference: "総会の日程（11月5日）を議事録に追記した。ほかに、タイムゾーンの設定と表示テーマを変えた。",
            key_facts: &["11月5日", "Asia/Tokyo", "dark"],
            changes: vec![
                paragraph_modified("議事録/総会.docx", &["総会は11月5日に開催する。"], &[]),
                text_modified(
                    "src/app.rs",
                    &["    let zone = \"Asia/Tokyo\";"],
                    &["    let zone = \"UTC\";"],
                ),
                text_modified("data/設定.json", &["  \"theme\": \"dark\""], &["  \"theme\": \"light\""]),
            ],
        },
        Case {
            id: "E03",
            category: "E",
            reference: "週報 15 件（10月1日〜15日）に、それぞれ出張の精算を済ませたことを 1 行ずつ追記した。",
            key_facts: &["出張", "精算"],
            changes: (1..=15)
                .map(|n| {
                    let path = format!("週報/2026-10-{n:02}.txt");
                    let line = format!("{n}日: 出張の精算を済ませた");
                    text_modified(&path, &[line.as_str()], &[])
                })
                .collect(),
        },
        Case {
            id: "E04",
            category: "E",
            reference: "返品期限を 14 日から 7 日に変更した（規約と FAQ）。ほかに、提出期限（10月31日）を追加し、在庫表の 1 セルを変え、古いチラシを削除した。",
            key_facts: &["返品", "10月31日"],
            changes: vec![
                paragraph_modified(
                    "契約/利用規約.docx",
                    &["返品は商品到着後7日以内に限る。"],
                    &["返品は商品到着後14日以内とする。"],
                ),
                text_modified("FAQ.md", &["返品期限は7日以内です。"], &["返品期限は14日以内です。"]),
                xlsx_change("在庫/在庫表.xlsx", &[cell("棚卸", "B7", Some("50"), Some("48"))]),
                text_added("締切/提出期限.txt", &["締切は10月31日（金）"]),
                bare(ChangeKind::Deleted, "旧資料/古いチラシ.txt"),
            ],
        },
        // F: 構造の変更（削除・名前変更・画像）
        Case {
            id: "F01",
            category: "F",
            reference: "古い資料フォルダの旧見積_2023.xlsx と旧議事録.docx を削除した。",
            key_facts: &[],
            changes: vec![
                bare(ChangeKind::Deleted, "古い資料/旧見積_2023.xlsx"),
                bare(ChangeKind::Deleted, "古い資料/旧議事録.docx"),
            ],
        },
        Case {
            id: "F02",
            category: "F",
            reference: "企画フォルダの案1.docx を、提出用フォルダに移した（名前は変えていない）。",
            key_facts: &[],
            changes: vec![renamed("企画/案1.docx", "提出用/案1.docx")],
        },
        Case {
            id: "F03",
            category: "F",
            reference: "会場図.png の画像を差し替え、ロゴ_新.png を新しく追加した（画像の中身は確認していない）。",
            key_facts: &[],
            changes: vec![
                bare(ChangeKind::Modified, "写真/会場図.png"),
                bare(ChangeKind::Added, "写真/ロゴ_新.png"),
            ],
        },
    ]
}

/// 入力の要約（評価ノートの表に載せる。変更ごとに種類・名前・変更量・差分の先頭 2 行）
fn change_summary(changes: &[SourceChange]) -> Vec<Value> {
    changes
        .iter()
        .map(|c| {
            let head: Vec<String> = c
                .diff_text
                .as_deref()
                .unwrap_or("")
                .lines()
                .take(2)
                .map(|l| l.chars().take(60).collect())
                .collect();
            json!({
                "kind": format!("{:?}", c.kind),
                "path": c.path,
                "old_path": c.old_path,
                "amount": c.amount,
                "diff_head": head,
            })
        })
        .collect()
}

/// 1 回の生成の検査の結果（`service.rs` の `finish` と同じ順序。内訳は `body_run`（12 節の計測）と同じ判定）
#[derive(Default)]
struct Analysis {
    memo: Option<Memo>,
    /// 解析の後に残った本文の行数（検査の前）
    candidates: usize,
    drop_sensitive: usize,
    drop_copy: usize,
    drop_ground: usize,
    /// 解析の段階で本文の行を捨てたか（形式・長さ・上限）
    parse_dropped: bool,
}

fn analyze(raw: &str, input: &ChangeInput) -> Analysis {
    let Some(parsed) = parse_memo(raw) else {
        return Analysis::default();
    };
    let corpus = input
        .files
        .iter()
        .filter_map(|file| file.diff_text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Analysis {
        candidates: parsed.body.len(),
        parse_dropped: parsed.body_dropped.is_some(),
        ..Analysis::default()
    };
    for line in &parsed.body {
        if looks_sensitive(line).is_some() {
            out.drop_sensitive += 1;
        } else if copies_diff(line, &corpus, 12) {
            out.drop_copy += 1;
        } else if screen_memo(
            Memo {
                title: "x".into(),
                body: vec![line.clone()],
                body_dropped: None,
            },
            input,
        )
        .is_some_and(|m| m.body.is_empty())
        {
            out.drop_ground += 1;
        }
    }
    out.memo = screen_memo(parsed, input);
    out
}

/// 1 入力の生成の結果（経路の記録を含む）
#[derive(Default)]
struct Pass {
    analysis: Analysis,
    /// 生成のエラー（あれば）。提案の理由（`failure_of`）と、エラーの内容
    error: Option<String>,
    failure: Option<String>,
    raw_first: String,
    raw_retry: Option<String>,
    retry_attempted: bool,
    retry_used: bool,
    prompt_tokens: Option<u64>,
    elapsed: Duration,
}

/// 1 回の要求。サーバーの起動（必要なら）から生成まで
async fn generate(
    server: &LlamaServer,
    chat: &ChatClient,
    path: &Path,
    messages: &[Message],
    timeout: Duration,
) -> Result<ChatOutput, LlmError> {
    let base = server.ensure_started(path).await?;
    chat.chat_with_usage(&base, MODEL_NAME, messages, timeout)
        .await
}

/// 内容を持つ変更が 1 件でもあるか（`core-ops` の `memo_has_content` と同じ規則。このテストは core-ops に依存しないため写す）。
/// 削除以外で、変更量か差分の本文を持つ変更を数える。
fn has_content(input: &ChangeInput) -> bool {
    input.files.iter().any(|file| {
        file.kind != ChangeKind::Deleted
            && (file.amount.is_some() || file.diff_text.as_deref().is_some_and(|t| !t.is_empty()))
    })
}

/// 現在の保存メモの生成の経路（`service.rs` の `suggest_with` と同じ順序。本文あり、上限は `GENERATE_TIMEOUT`）
async fn suggest_pipeline(
    server: &LlamaServer,
    chat: &ChatClient,
    path: &Path,
    input: &ChangeInput,
) -> Pass {
    let started = Instant::now();
    let mut pass = Pass::default();
    // 内容を持つ変更が無ければ LLM を呼ばない（`app` の `suggest_for_project` と同じ。設計 10.4）
    if !has_content(input) {
        pass.failure = Some("NoContent".to_string());
        pass.elapsed = started.elapsed();
        return pass;
    }
    let first = match generate(
        server,
        chat,
        path,
        &build_messages(input, true),
        GENERATE_TIMEOUT,
    )
    .await
    {
        Ok(out) => out,
        Err(e) => {
            pass.error = Some(format!("{e:?}"));
            pass.failure = Some(format!("{:?}", failure_of(e, false)));
            pass.elapsed = started.elapsed();
            return pass;
        }
    };
    pass.prompt_tokens = first.prompt_tokens;
    pass.raw_first = first.content.clone();
    let mut chosen = analyze(&first.content, input);
    // 件数だけで内容に触れていない要約は、残りの時間の中で 1 回だけ書き直させる（検証で不採用なら書き直さない）
    let needs_retry = chosen
        .memo
        .as_ref()
        .is_some_and(|m| needs_content_retry(input, &m.title));
    if needs_retry {
        let left = GENERATE_TIMEOUT.saturating_sub(started.elapsed());
        if !left.is_zero() {
            pass.retry_attempted = true;
            if let Ok(out) = generate(
                server,
                chat,
                path,
                &content_retry_messages(input, true),
                left,
            )
            .await
            {
                let second = analyze(&out.content, input);
                let accepted = second
                    .memo
                    .as_ref()
                    .is_some_and(|m| !needs_content_retry(input, &m.title));
                pass.raw_retry = Some(out.content);
                if accepted {
                    pass.retry_used = true;
                    chosen = second;
                }
            }
        }
    }
    pass.analysis = chosen;
    pass.elapsed = started.elapsed();
    pass
}

/// 語の一致（内容ヒット）
fn has_key(text: &str, keys: &[&str]) -> bool {
    keys.iter().any(|key| text.contains(key))
}

/// 要約が定型語だけで書けているか（ファイル名・件数・操作の語・助詞・記号を除いて、残りが無い）
fn is_template_only(text: &str, input: &ChangeInput) -> bool {
    let mut names: Vec<String> = Vec::new();
    for file in &input.files {
        for name in std::iter::once(file.name.as_str()).chain(file.old_name.as_deref()) {
            names.push(name.to_string());
            if let Some((stem, _)) = name.rsplit_once('.') {
                if !stem.is_empty() {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort_by_key(|n| Reverse(n.len()));
    let mut rest = text.to_string();
    for name in &names {
        rest = rest.replace(name.as_str(), " ");
    }
    let mut words: Vec<&str> = TEMPLATE_WORDS.to_vec();
    words.sort_by_key(|w| Reverse(w.len()));
    for word in words {
        rest = rest.replace(word, " ");
    }
    rest.chars().all(|c| {
        c.is_whitespace()
            || c.is_ascii_digit()
            || c.is_ascii_punctuation()
            || "（）「」・：".contains(c)
    })
}

/// 1 回分の記録
struct Record {
    id: &'static str,
    category: &'static str,
    run: usize,
    title: Option<String>,
    body: Vec<String>,
    failure: Option<String>,
    error: Option<String>,
    notices: Vec<String>,
    candidates: usize,
    drop_sensitive: usize,
    drop_copy: usize,
    drop_ground: usize,
    parse_dropped: bool,
    retry_attempted: bool,
    retry_used: bool,
    elapsed_ms: u128,
    prompt_tokens: Option<u64>,
    raw_first: String,
    raw_retry: Option<String>,
    content_hit: Option<bool>,
    template_only: bool,
    names_only: bool,
    input_summary: Vec<Value>,
    reference: &'static str,
    key_facts: &'static [&'static str],
}

impl Record {
    fn passed(&self) -> bool {
        self.title.is_some()
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "category": self.category,
            "run": self.run,
            "passed": self.passed(),
            "failure": self.failure,
            "error": self.error,
            "title": self.title,
            "body": self.body,
            "notices": self.notices,
            "body_lines": self.body.len(),
            "candidates": self.candidates,
            "drop": {
                "sensitive": self.drop_sensitive,
                "copy": self.drop_copy,
                "ground": self.drop_ground,
                "parse": self.parse_dropped,
            },
            "retry_attempted": self.retry_attempted,
            "retry_used": self.retry_used,
            "elapsed_ms": self.elapsed_ms,
            "prompt_tokens": self.prompt_tokens,
            "content_hit": self.content_hit,
            "template_only": self.template_only,
            "names_only": self.names_only,
            "raw_first": self.raw_first,
            "raw_retry": self.raw_retry,
            "input_summary": self.input_summary,
            "reference": self.reference,
            "key_facts": self.key_facts,
        })
    }
}

fn record_of(
    case: &Case,
    run: usize,
    input: &ChangeInput,
    notices: &[SuggestNotice],
    pass: Pass,
) -> Record {
    let title = pass.analysis.memo.as_ref().map(|m| m.title.clone());
    let body = pass
        .analysis
        .memo
        .as_ref()
        .map(|m| m.body.clone())
        .unwrap_or_default();
    // 生成のエラーは理由に変換済み（`suggest_pipeline`）。エラーが無く要約も無いものは検証で不採用
    let failure = match (&pass.failure, &title) {
        (Some(kind), _) => Some(kind.clone()),
        (None, None) => Some("Invalid".to_string()),
        (None, Some(_)) => None,
    };
    let mut hit_text = title.clone().unwrap_or_default();
    for line in &body {
        hit_text.push('\n');
        hit_text.push_str(line);
    }
    // 要点が無い入力は内容を問わない（分母から外す）。不採用・失敗は内容ヒットしないものとして数える
    let content_hit = if case.key_facts.is_empty() {
        None
    } else {
        Some(title.is_some() && has_key(&hit_text, case.key_facts))
    };
    let template_only = title.as_deref().is_some_and(|t| is_template_only(t, input));
    let names_only = template_only && content_hit != Some(true);
    Record {
        id: case.id,
        category: case.category,
        run,
        title,
        body,
        failure,
        error: pass.error.clone(),
        notices: notices
            .iter()
            .map(|n| format!("{n:?}"))
            // 本文を捨てた理由（`app` の `suggest_for_project` が出す注記と同じ対応）
            .chain(
                pass.analysis
                    .memo
                    .as_ref()
                    .and_then(|m| m.body_dropped)
                    .map(|reason| format!("{:?}", SuggestNotice::from_body_drop(reason))),
            )
            .collect(),
        candidates: pass.analysis.candidates,
        drop_sensitive: pass.analysis.drop_sensitive,
        drop_copy: pass.analysis.drop_copy,
        drop_ground: pass.analysis.drop_ground,
        parse_dropped: pass.analysis.parse_dropped,
        retry_attempted: pass.retry_attempted,
        retry_used: pass.retry_used,
        elapsed_ms: pass.elapsed.as_millis(),
        prompt_tokens: pass.prompt_tokens,
        raw_first: pass.raw_first,
        raw_retry: pass.raw_retry,
        content_hit,
        template_only,
        names_only,
        input_summary: change_summary(&case.changes),
        reference: case.reference,
        key_facts: case.key_facts,
    }
}

/// 集計（全体・カテゴリごとに同じ式で数える）
fn summarize(records: &[&Record]) -> Value {
    let n = records.len();
    let passed = records.iter().filter(|r| r.passed()).count();
    let scorable: Vec<&&Record> = records.iter().filter(|r| r.content_hit.is_some()).collect();
    let hits = scorable
        .iter()
        .filter(|r| r.content_hit == Some(true))
        .count();
    let names_only = records.iter().filter(|r| r.names_only).count();
    let passed_recs: Vec<&&Record> = records.iter().filter(|r| r.passed()).collect();
    let template_only = passed_recs.iter().filter(|r| r.template_only).count();
    let with_body = records.iter().filter(|r| !r.body.is_empty()).count();
    let body_lines_sum: usize = passed_recs.iter().map(|r| r.body.len()).sum();
    let mut times: Vec<u128> = records.iter().map(|r| r.elapsed_ms).collect();
    times.sort_unstable();
    let median = times.get(times.len() / 2).copied().unwrap_or(0);
    let max = times.last().copied().unwrap_or(0);
    let timeouts = records
        .iter()
        .filter(|r| r.failure.as_deref() == Some("Timeout"))
        .count();
    let no_content = records
        .iter()
        .filter(|r| r.failure.as_deref() == Some("NoContent"))
        .count();
    let retried = records.iter().filter(|r| r.retry_attempted).count();
    let retry_used = records.iter().filter(|r| r.retry_used).count();
    let drop_total = |f: fn(&Record) -> usize| -> usize { records.iter().map(|r| f(r)).sum() };
    json!({
        "runs": n,
        "passed": passed,
        "passed_rate": rate(passed, n),
        "content_hit": hits,
        "content_scorable": scorable.len(),
        "content_hit_rate": rate(hits, scorable.len()),
        "names_only": names_only,
        "names_only_rate": rate(names_only, n),
        "template_only_summary": template_only,
        "template_only_rate": rate(template_only, passed_recs.len()),
        "with_body": with_body,
        "with_body_rate": rate(with_body, n),
        "body_lines_avg": if passed_recs.is_empty() { 0.0 } else { body_lines_sum as f64 / passed_recs.len() as f64 },
        "elapsed_ms_median": median,
        "elapsed_ms_max": max,
        "timeouts": timeouts,
        "retry_attempted": retried,
        "retry_used": retry_used,
        "drop_sensitive": drop_total(|r| r.drop_sensitive),
        "drop_copy": drop_total(|r| r.drop_copy),
        "drop_ground": drop_total(|r| r.drop_ground),
        // LLM を呼ばなかった件数（内容の無い変更。`NoContent`）。検証通過などの率は、LLM を呼んだ件数で割る
        "no_content": no_content,
        "llm_calls": n - no_content,
    })
}

fn rate(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

/// 環境変数からサーバーの接続情報を読む。未設定なら `None`（テストは省略する）
fn server_env() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::var_os("HIKAE_TEST_LLAMA_SERVER")?;
    let model = std::env::var_os("HIKAE_TEST_MODEL")?;
    Some((PathBuf::from(exe), PathBuf::from(model)))
}

/// 32 件の構成（カテゴリごとの件数、ID の重複なし、参考文と要点の記入、要点がファイル名に含まれないこと）を確かめる。サーバーは使わない
#[test]
fn realistic_inputs_match_the_planned_set() {
    let all = cases();
    assert_eq!(all.len(), 32);
    let count = |c: &str| all.iter().filter(|case| case.category == c).count();
    assert_eq!(
        [
            count("A"),
            count("B"),
            count("C"),
            count("D"),
            count("E"),
            count("F")
        ],
        [8, 6, 5, 6, 4, 3]
    );
    let mut ids: Vec<&str> = all.iter().map(|case| case.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 32, "ID が重複している");
    for case in &all {
        assert!(!case.reference.is_empty(), "{}: 参考文が無い", case.id);
        assert!(!case.changes.is_empty(), "{}: 変更が無い", case.id);
        let prepared = prepare_input(&case.changes);
        assert!(!prepared.input.files.is_empty(), "{}", case.id);
        // 要点の語がファイル名に含まれていると、名前だけの出力が内容ヒットになってしまうため
        for key in case.key_facts {
            for change in &case.changes {
                let in_name = std::iter::once(change.path.as_str())
                    .chain(change.old_path.as_deref())
                    .any(|p| p.contains(key));
                assert!(!in_name, "{}: 要点 {key} がファイル名に含まれる", case.id);
            }
        }
    }
}

#[tokio::test]
#[ignore = "実機の llama-server と GGUF が必要。現実的な入力 32 件でのモデル比較（HIKAE_TEST_RUNS 回ずつ。既定 1）"]
async fn compare_realistic_inputs() {
    let Some((exe, model)) = server_env() else {
        eprintln!("環境変数が未設定のため省略");
        return;
    };
    assert!(model.is_file(), "モデルファイルがありません");
    let label = std::env::var("HIKAE_TEST_MODEL_LABEL").unwrap_or_else(|_| "unknown".into());
    let runs: usize = std::env::var("HIKAE_TEST_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    let options = LlamaServerOptions::new(Some(exe));
    let threads = options.threads;
    let server = LlamaServer::new(options).unwrap();
    let chat = ChatClient::new().unwrap();
    let all = cases();

    // ウォームアップ（記録しない）。モデルの読み込みを計測から外す
    let warm = prepare_input(&all[0].changes);
    let _ = generate(
        &server,
        &chat,
        &model,
        &build_messages(&warm.input, true),
        GENERATE_TIMEOUT,
    )
    .await;

    let mut records: Vec<Record> = Vec::new();
    for case in &all {
        let prepared = prepare_input(&case.changes);
        for run in 0..runs {
            let pass = suggest_pipeline(&server, &chat, &model, &prepared.input).await;
            let record = record_of(case, run, &prepared.input, &prepared.notices, pass);
            println!(
                "[{label}] {} run {run}: {} ms passed={} failure={:?} retry={}/{} title={:?} body={:?}",
                record.id,
                record.elapsed_ms,
                record.passed(),
                record.failure,
                record.retry_attempted,
                record.retry_used,
                record.title,
                record.body
            );
            records.push(record);
        }
    }
    server.stop().await;
    assert!(!server.is_running().await, "llama-server が残っている");

    // 同じ入力の出力が一致する割合（runs が 2 以上のとき）
    let mut same = 0usize;
    let mut pairs = 0usize;
    if runs >= 2 {
        for case in &all {
            let outs: Vec<(Option<&String>, &Vec<String>)> = records
                .iter()
                .filter(|r| r.id == case.id)
                .map(|r| (r.title.as_ref(), &r.body))
                .collect();
            if outs.len() >= 2 {
                pairs += 1;
                if outs[0] == outs[1] {
                    same += 1;
                }
            }
        }
    }

    let all_refs: Vec<&Record> = records.iter().collect();
    let mut by_category = serde_json::Map::new();
    for c in ["A", "B", "C", "D", "E", "F"] {
        let subset: Vec<&Record> = records.iter().filter(|r| r.category == c).collect();
        by_category.insert(c.into(), summarize(&subset));
    }
    let overall = summarize(&all_refs);
    let report = json!({
        "model": label,
        "model_file": model.file_name().map(|s| s.to_string_lossy().into_owned()),
        "threads": threads,
        "runs": runs,
        "determinism": { "pairs": pairs, "same": same },
        "overall": overall,
        "by_category": by_category,
        "records": records.iter().map(|r| r.to_json()).collect::<Vec<_>>(),
    });
    println!(
        "[{label}] 全体: {}",
        serde_json::to_string(&report["overall"]).unwrap()
    );
    if let Some(out) = std::env::var_os("HIKAE_TEST_OUT") {
        std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!(
            "[{label}] 結果を書き出した: {}",
            PathBuf::from(out).display()
        );
    }
    assert_eq!(records.len(), all.len() * runs);
}
