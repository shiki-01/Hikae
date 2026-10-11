// 入力の構造化とプロンプト生成（設計 10.4 の 2）。評価レポートの形式（変更ファイルの表＋差分の抜粋）に従う。
// 入力のパス・本文はログに出さない。このモジュールは文字列を組み立てるだけ。

use serde::Serialize;
use std::fmt::Write as _;

/// システムプロンプト（評価ノート 11.5 の V1 の文面。内容を最優先にする指示と、良い例・悪い例 3 組）。
/// 例は評価の入力と同じ内容を避けた別の内容にしている。文面を変えたら評価を取り直すこと。
pub const SYSTEM_PROMPT: &str =
    "あなたは、ファイルの変更内容から「保存メモ」を書く assistant です。
最優先の規則: 何のファイルかではなく、「何がどう変わったか」（変更前後の値、追加・削除された内容、変更の目的）を書く。ファイル名は、内容を説明するのに必要なときだけ短く添える。「〜を更新した」「〜を修正した」のようにファイル名と操作だけの文は書かない。
その他の規則:
- 出力は日本語で 1 文のみ。60 文字程度まで。
- 複数のファイルが同じ目的で変わっているときは、その目的と変更内容（例: 税率を 8% から 10% に変えた）を 1 文で書く。無関係な変更が混ざるときは、主な変更の内容を 2 つ程度まで書く。
- 変更ファイルが 10 件を超えるときは、ファイル名を並べず、種類と件数でまとめる。
- 与えられた情報にある事実だけを書く。差分や名前から分からない内容（画像の中身など）は推測せず、「画像を差し替えた」のように事実だけを書く。
- 専門用語（commit、diff、push、branch、repository など）は使わない。
- 前置き、説明、引用符、記号の装飾は付けない。メモの文だけを出力する。

例1
変更: 契約書ひな形.docx（更新）: - 支払は月末締め翌月末払いとする。 → + 支払は月末締め翌々月 15 日払いとする。
良い例: 契約書の支払期限を、翌月末払いから翌々月15日払いに変えた。
悪い例: 契約書ひな形.docx を更新した。

例2
変更: 在庫表.xlsx（更新）: 在庫!C3(商品A): 40 → 25、在庫!C7(商品C): 10 → 0
良い例: 在庫表で商品Aを40個から25個に、商品Cを10個から0個に減らした。
悪い例: 在庫表.xlsx のセルを更新した。

例3
変更: login.py（更新）: - if len(password) < 6: → + if len(password) < 8:
良い例: ログイン時に使えるパスワードを、6文字以上から8文字以上に厳しくした。
悪い例: login.py を修正した。";

/// 表に載せるファイルの上限。超えた分は「ほか n 件」にまとめる
pub const MAX_FILES: usize = 30;
/// 差分本文を渡すファイルの数（変更量の多い順。それ以外は表の行だけ）
pub const MAX_DIFF_FILES: usize = 3;
/// 1 ファイルの差分本文の上限（文字数）
pub const MAX_FILE_DIFF_CHARS: usize = 500;
/// 差分本文の全体の上限（文字数）
pub const MAX_TOTAL_DIFF_CHARS: usize = 1500;
/// 表に出すファイル名の上限（文字数）。超えた分は「…」で省略する
pub const MAX_NAME_CHARS: usize = 60;

const OMITTED_NOTE: &str = "（以下省略）";
const USER_LEAD: &str = "次の変更の保存メモを書いてください。";
/// ユーザーメッセージの末尾に添える指示（評価ノート 11.5 の V1 の末尾。何がどう変わったかを 1 文で書かせる）
const USER_TAIL: &str =
    "何のファイルかではなく、何がどう変わったか（値・内容・目的）を 1 文で書いてください。ファイル名は必要なときだけ添えてください。";

/// 変更の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl ChangeKind {
    fn label(self) -> &'static str {
        match self {
            ChangeKind::Added => "追加",
            ChangeKind::Modified => "更新",
            ChangeKind::Deleted => "削除",
            ChangeKind::Renamed => "名前変更",
        }
    }
}

/// 変更された 1 ファイル。`amount` は「段落 +2 / -1」のような変更量の要約（呼び出し側が作る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub kind: ChangeKind,
    pub name: String,
    /// 名前変更の旧名
    pub old_name: Option<String>,
    pub amount: Option<String>,
    /// 抽出したテキストの差分（テキスト・docx・xlsx など）
    pub diff_text: Option<String>,
}

/// モデルへ渡す入力（変更ファイルの一覧）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeInput {
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
}

/// チャット形式のメッセージ
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// システムプロンプトとユーザーメッセージの 2 件を作る。
pub fn build_messages(input: &ChangeInput) -> Vec<Message> {
    vec![
        Message {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
        },
        Message {
            role: Role::User,
            content: user_message(input),
        },
    ]
}

fn user_message(input: &ChangeInput) -> String {
    let shown_count = input.files.len().min(MAX_FILES);
    let shown = &input.files[..shown_count];

    let mut out = String::new();
    out.push_str(USER_LEAD);
    out.push_str("\n\n変更ファイル:\n| 種別 | ファイル名 | 変更量 |\n| --- | --- | --- |\n");
    for file in shown {
        let name = match (&file.kind, &file.old_name) {
            (ChangeKind::Renamed, Some(old)) => {
                format!("{} → {}", short_name(old), short_name(&file.name))
            }
            _ => short_name(&file.name),
        };
        let amount = file.amount.as_deref().unwrap_or("-");
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            file.kind.label(),
            table_cell(&name),
            table_cell(amount)
        );
    }
    if input.files.len() > shown_count {
        let _ = writeln!(out, "ほか {} 件", input.files.len() - shown_count);
    }
    if input.files.len() > COUNT_SUMMARY_FROM {
        let _ = writeln!(out, "件数: {}", kind_counts(&input.files));
    }

    // 差分本文は、変更量の多い上位のファイルだけ（表の順に並べる）
    let bodies = body_indices(shown);
    let mut budget = MAX_TOTAL_DIFF_CHARS;
    let mut diffs = String::new();
    for (idx, file) in shown.iter().enumerate() {
        if !bodies.contains(&idx) {
            continue;
        }
        let Some(text) = file.diff_text.as_deref() else {
            continue;
        };
        if budget == 0 {
            diffs.push_str(OMITTED_NOTE);
            diffs.push('\n');
            break;
        }
        let limit = budget.min(MAX_FILE_DIFF_CHARS);
        let (body, truncated) = truncate_chars(text, limit);
        budget -= body.chars().count();
        let _ = writeln!(diffs, "[{}]", short_name(&file.name));
        diffs.push_str(&body);
        diffs.push('\n');
        if truncated {
            diffs.push_str(OMITTED_NOTE);
            diffs.push('\n');
        }
        diffs.push('\n');
    }
    if !diffs.is_empty() {
        out.push_str("\n差分（抽出テキスト）:\n");
        out.push_str(&diffs);
    }
    out.push('\n');
    out.push_str(USER_TAIL);
    out.push('\n');
    out
}

/// 差分本文を渡す表の位置（昇順）。差分の行数（空行を除く）が多い順に `MAX_DIFF_FILES` 件。
/// 同じ行数のときは表の順を優先する。差分が無いファイルは対象にしない。
fn body_indices(shown: &[ChangedFile]) -> Vec<usize> {
    let mut ranked: Vec<(usize, usize)> = shown
        .iter()
        .enumerate()
        .filter_map(|(idx, file)| {
            let lines = file
                .diff_text
                .as_deref()?
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            (lines > 0).then_some((idx, lines))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.truncate(MAX_DIFF_FILES);
    let mut picked: Vec<usize> = ranked.into_iter().map(|(idx, _)| idx).collect();
    picked.sort_unstable();
    picked
}

/// 変更ファイルの数がこれを超えるとき（5 件以上）、表の後に種別ごとの件数を添える。
/// これは入力の事実の補足で（表の全件を数えた値）、出力の方針（SYSTEM_PROMPT の「10 件を超えるとき件数でまとめる」）とは別に働く。
/// 評価（ノート 11.4）と同じ条件のため、閾値は変えていない。
const COUNT_SUMMARY_FROM: usize = 4;

/// 種別ごとの件数（「更新 28 件、追加 7 件」）。0 件の種別は出さない。件数は全件で数える
fn kind_counts(files: &[ChangedFile]) -> String {
    [
        ChangeKind::Added,
        ChangeKind::Modified,
        ChangeKind::Deleted,
        ChangeKind::Renamed,
    ]
    .iter()
    .filter_map(|kind| {
        let count = files.iter().filter(|f| f.kind == *kind).count();
        (count > 0).then(|| format!("{} {count} 件", kind.label()))
    })
    .collect::<Vec<_>>()
    .join("、")
}

/// 表示用のファイル名。`MAX_NAME_CHARS` 文字を超える分は「…」で省略する（文字境界で切る）。
fn short_name(name: &str) -> String {
    if name.chars().count() <= MAX_NAME_CHARS {
        return name.to_string();
    }
    let head: String = name.chars().take(MAX_NAME_CHARS - 1).collect();
    format!("{head}…")
}

/// 表のセルに入れられるように、区切り記号と改行を空白に置き換える。
fn table_cell(s: &str) -> String {
    s.replace(['|', '\r', '\n'], " ")
}

/// 先頭から `max` 文字までを取り出す。文字境界で切るため、途中で壊れない。戻り値は（本文, 切ったかどうか）。
fn truncate_chars(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((idx, _)) => (text[..idx].to_string(), true),
        None => (text.to_string(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, diff: Option<&str>) -> ChangedFile {
        ChangedFile {
            kind: ChangeKind::Modified,
            name: name.into(),
            old_name: None,
            amount: Some("段落 +1 / -0".into()),
            diff_text: diff.map(str::to_string),
        }
    }

    fn user_content(input: &ChangeInput) -> String {
        build_messages(input).remove(1).content
    }

    #[test]
    fn builds_system_and_user_messages() {
        let input = ChangeInput {
            files: vec![file("報告書.docx", Some("+ 追加した段落"))],
        };
        let messages = build_messages(&input);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[0].content, SYSTEM_PROMPT);
        assert_eq!(messages[1].role, Role::User);
        let user = &messages[1].content;
        assert!(user.starts_with("次の変更の保存メモを書いてください。"));
        assert!(user.contains("| 更新 | 報告書.docx | 段落 +1 / -0 |"));
        assert!(user.contains("[報告書.docx]\n+ 追加した段落"));
    }

    #[test]
    fn renamed_shows_old_and_new_name_and_missing_amount_is_dash() {
        let input = ChangeInput {
            files: vec![ChangedFile {
                kind: ChangeKind::Renamed,
                name: "発表資料_最終版.pptx".into(),
                old_name: Some("発表資料_仮.pptx".into()),
                amount: None,
                diff_text: None,
            }],
        };
        let user = user_content(&input);
        assert!(user.contains("| 名前変更 | 発表資料_仮.pptx → 発表資料_最終版.pptx | - |"));
        assert!(!user.contains("差分（抽出テキスト）"));
    }

    #[test]
    fn table_cells_do_not_break_the_table() {
        let input = ChangeInput {
            files: vec![file("a|b\nc.txt", None)],
        };
        let user = user_content(&input);
        assert!(user.contains("| 更新 | a b c.txt | 段落 +1 / -0 |"));
    }

    #[test]
    fn truncates_each_file_at_500_chars_with_note() {
        let long = "あ".repeat(900);
        let input = ChangeInput {
            files: vec![file("a.txt", Some(&long))],
        };
        let user = user_content(&input);
        let body = "あ".repeat(500);
        assert!(user.contains(&format!("[a.txt]\n{body}\n{OMITTED_NOTE}\n")));
        assert!(!user.contains(&"あ".repeat(501)));
    }

    #[test]
    fn only_top_three_files_by_diff_lines_get_bodies() {
        // 差分の行数（空行を除く）: f0=1, f1=5, f2=3, f3=4, f4=2 → 本文は f1・f2・f3 だけ（表の順）
        let lines = [1usize, 5, 3, 4, 2];
        let files: Vec<ChangedFile> = lines
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let text = (0..n).map(|k| format!("+ 行{k}\n")).collect::<String>();
                file(&format!("f{i}.txt"), Some(&text))
            })
            .collect();
        let user = user_content(&ChangeInput { files });
        assert!(!user.contains("[f0.txt]"));
        assert!(!user.contains("[f4.txt]"));
        let f1 = user.find("[f1.txt]").unwrap();
        let f2 = user.find("[f2.txt]").unwrap();
        let f3 = user.find("[f3.txt]").unwrap();
        assert!(f1 < f2 && f2 < f3, "表の順に並ぶこと");
        // 表には全件の行が残る
        assert!(user.contains("| 更新 | f0.txt |"));
    }

    #[test]
    fn total_diff_is_capped_at_1500_chars_across_three_files() {
        let text = "い".repeat(800);
        let files: Vec<ChangedFile> = (0..10)
            .map(|i| file(&format!("f{i}.txt"), Some(&text)))
            .collect();
        let user = user_content(&ChangeInput { files });
        // 差分の節だけを数える（末尾の指示文にも「い」が含まれるため）
        let diffs = user
            .split("差分（抽出テキスト）:\n")
            .nth(1)
            .unwrap_or_default()
            .split(USER_TAIL)
            .next()
            .unwrap_or_default();
        let total: usize = diffs.chars().filter(|c| *c == 'い').count();
        assert_eq!(total, MAX_TOTAL_DIFF_CHARS);
        // 本文は同じ行数の先頭 3 件（表の順）。それぞれ 500 文字で切り、省略の注記を付ける
        assert_eq!(user.matches(OMITTED_NOTE).count(), 3);
        assert!(user.contains("[f0.txt]"));
        assert!(user.contains("[f2.txt]"));
        assert!(!user.contains("[f3.txt]"));
        assert!(!user.contains("[f9.txt]"));
    }

    #[test]
    fn long_names_are_shortened_to_60_chars_with_ellipsis() {
        let long = format!("{}.docx", "あ".repeat(70));
        let input = ChangeInput {
            files: vec![file(&long, None)],
        };
        let user = user_content(&input);
        let short = format!("{}…", "あ".repeat(MAX_NAME_CHARS - 1));
        assert!(user.contains(&format!("| 更新 | {short} | 段落 +1 / -0 |")));
        assert!(!user.contains(&"あ".repeat(MAX_NAME_CHARS)));
    }

    #[test]
    fn renamed_names_are_shortened_each() {
        let long = "い".repeat(65);
        let input = ChangeInput {
            files: vec![ChangedFile {
                kind: ChangeKind::Renamed,
                name: format!("{long}_新"),
                old_name: Some("短い旧名.docx".into()),
                amount: None,
                diff_text: None,
            }],
        };
        let user = user_content(&input);
        let new_short = format!("{}…", "い".repeat(MAX_NAME_CHARS - 1));
        assert!(user.contains(&format!("| 名前変更 | 短い旧名.docx → {new_short} | - |")));
    }

    #[test]
    fn user_message_ends_with_the_one_sentence_instruction() {
        let files: Vec<ChangedFile> = (0..3).map(|i| file(&format!("f{i}.txt"), None)).collect();
        let user = user_content(&ChangeInput { files });
        assert!(user.trim_end().ends_with(USER_TAIL));
    }

    #[test]
    fn system_prompt_asks_for_content_first_and_one_sentence() {
        // V1（評価ノート 11.5）の要点: 内容を最優先にし、ファイル名だけの文を禁じ、1 文に収める
        assert!(
            SYSTEM_PROMPT.contains("最優先の規則: 何のファイルかではなく、「何がどう変わったか」")
        );
        assert!(SYSTEM_PROMPT.contains("ファイル名と操作だけの文は書かない"));
        assert!(SYSTEM_PROMPT.contains("出力は日本語で 1 文のみ。60 文字程度まで。"));
        // 件数でまとめる閾値は 10 件超（V1）
        assert!(SYSTEM_PROMPT.contains("変更ファイルが 10 件を超えるとき"));
        assert!(!SYSTEM_PROMPT.contains("3 件を超える"));
        // 評価の入力と同じ内容の例は含めない（例は V1 の 3 組のまま）
        assert_eq!(SYSTEM_PROMPT.matches("良い例:").count(), 3);
        assert_eq!(SYSTEM_PROMPT.matches("悪い例:").count(), 3);
    }

    #[test]
    fn user_tail_is_the_v1_instruction() {
        assert_eq!(
            USER_TAIL,
            "何のファイルかではなく、何がどう変わったか（値・内容・目的）を 1 文で書いてください。ファイル名は必要なときだけ添えてください。"
        );
    }

    #[test]
    fn short_name_keeps_names_at_the_limit_and_cuts_at_char_boundary() {
        let exact = "あ".repeat(MAX_NAME_CHARS);
        assert_eq!(short_name(&exact), exact);
        let over = "𠀋".repeat(MAX_NAME_CHARS + 1);
        let cut = short_name(&over);
        assert_eq!(cut.chars().count(), MAX_NAME_CHARS);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn more_than_30_files_are_summarized_and_extra_are_not_listed() {
        let files: Vec<ChangedFile> = (0..33).map(|i| file(&format!("f{i}.txt"), None)).collect();
        let user = user_content(&ChangeInput { files });
        assert!(user.contains("| 更新 | f29.txt |"));
        assert!(!user.contains("| 更新 | f30.txt |"));
        assert!(user.contains("ほか 3 件"));
    }

    #[test]
    fn summary_line_counts_kinds_only_when_many_files() {
        let mut files: Vec<ChangedFile> =
            (0..6).map(|i| file(&format!("f{i}.txt"), None)).collect();
        files[0].kind = ChangeKind::Added;
        files[1].kind = ChangeKind::Deleted;
        let user = user_content(&ChangeInput { files });
        assert!(user.contains(
            "件数: 追加 1 件、更新 4 件、削除 1 件
"
        ));
        // 件数が少ないときは出さない
        let few: Vec<ChangedFile> = (0..3).map(|i| file(&format!("f{i}.txt"), None)).collect();
        assert!(!user_content(&ChangeInput { files: few }).contains("件数:"));
    }

    #[test]
    fn exactly_30_files_has_no_summary_line() {
        let files: Vec<ChangedFile> = (0..30).map(|i| file(&format!("f{i}.txt"), None)).collect();
        let user = user_content(&ChangeInput { files });
        assert!(!user.contains("ほか"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        // サロゲートペア（4 バイト文字）の途中で切れないこと
        let text = "𠀋".repeat(10);
        let (body, truncated) = truncate_chars(&text, 3);
        assert_eq!(body.chars().count(), 3);
        assert!(truncated);
        let (whole, not_truncated) = truncate_chars(&text, 10);
        assert_eq!(whole, text);
        assert!(!not_truncated);
    }

    #[test]
    fn blank_diff_is_skipped() {
        let input = ChangeInput {
            files: vec![file("memo.txt", Some("  \n "))],
        };
        let user = user_content(&input);
        assert!(!user.contains("差分（抽出テキスト）"));
    }
}
