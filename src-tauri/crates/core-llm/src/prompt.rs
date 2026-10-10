// 入力の構造化とプロンプト生成（設計 10.4 の 2）。評価レポートの形式（変更ファイルの表＋差分の抜粋）に従う。
// 入力のパス・本文はログに出さない。このモジュールは文字列を組み立てるだけ。

use serde::Serialize;
use std::fmt::Write as _;

/// システムプロンプト（評価レポート 1 節の文面）
pub const SYSTEM_PROMPT: &str =
    "あなたは、ファイルの変更内容から「保存メモ」を書く assistant です。
規則:
- 出力は日本語で 1 文のみ。40 文字程度まで。
- 与えられた情報にある事実だけを書く。推測や評価は書かない。
- 専門用語（commit、diff、push、branch、repository など）は使わない。
- ファイル名や、変更前後の具体的な値が分かる場合は、それを使う。
- 前置き、説明、引用符、記号の装飾は付けない。メモの文だけを出力する。";

/// 表に載せるファイルの上限。超えた分は「ほか n 件」にまとめる
pub const MAX_FILES: usize = 30;
/// 1 ファイルの差分本文の上限（文字数）
pub const MAX_FILE_DIFF_CHARS: usize = 800;
/// 差分本文の全体の上限（文字数）
pub const MAX_TOTAL_DIFF_CHARS: usize = 3000;

const OMITTED_NOTE: &str = "（以下省略）";
const USER_LEAD: &str = "次の変更の保存メモを書いてください。";

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
            (ChangeKind::Renamed, Some(old)) => format!("{old} → {}", file.name),
            _ => file.name.clone(),
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

    let mut budget = MAX_TOTAL_DIFF_CHARS;
    let mut diffs = String::new();
    for file in shown {
        let Some(text) = file.diff_text.as_deref().filter(|t| !t.trim().is_empty()) else {
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
        let _ = writeln!(diffs, "[{}]", file.name);
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
    out
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
    fn truncates_each_file_at_800_chars_with_note() {
        let long = "あ".repeat(900);
        let input = ChangeInput {
            files: vec![file("a.txt", Some(&long))],
        };
        let user = user_content(&input);
        let body = "あ".repeat(800);
        assert!(user.contains(&format!("[a.txt]\n{body}\n{OMITTED_NOTE}\n")));
        assert!(!user.contains(&"あ".repeat(801)));
    }

    #[test]
    fn total_diff_is_capped_at_3000_chars() {
        let text = "い".repeat(800);
        let files: Vec<ChangedFile> = (0..10)
            .map(|i| file(&format!("f{i}.txt"), Some(&text)))
            .collect();
        let user = user_content(&ChangeInput { files });
        let total: usize = user
            .split("差分（抽出テキスト）:\n")
            .nth(1)
            .unwrap_or_default()
            .chars()
            .filter(|c| *c == 'い')
            .count();
        assert_eq!(total, MAX_TOTAL_DIFF_CHARS);
        // 予算を使い切った後のファイルは本文を含まず、省略の注記を 1 回だけ付ける
        assert_eq!(user.matches(OMITTED_NOTE).count(), 2);
        assert!(!user.contains("[f9.txt]"));
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
