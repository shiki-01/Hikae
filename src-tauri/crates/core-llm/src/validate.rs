// モデルの出力の検証（設計 10.4）。条件を満たさない出力は採用せず `None` を返す（ルールベースのまま）。
// 純関数のみ。内容はログに出さない。

/// 1 行の上限（文字数）。評価で、事実として正しい出力が 80 文字を超える例（xlsx の複数セルの変更で 87 文字）が
/// あったため 100 文字とする（プロンプトの「40 文字程度」は小型モデルが守らない）
const MAX_CHARS: usize = 100;

/// 文の区切りとみなす記号。2 つ以上あれば複数文とみなす
const SENTENCE_END: char = '。';

/// Git 用語（英字は大文字小文字を区別せず、単語の先頭で一致を見る）
const GIT_TERMS_ASCII: [&str; 7] = [
    "commit",
    "diff",
    "push",
    "pull",
    "branch",
    "repository",
    "merge",
];

/// Git 用語（日本語）。「差分」は画面の用語（「変更差分を表示」）として使うため含めない
const GIT_TERMS_JA: [&str; 5] = ["コミット", "ブランチ", "プッシュ", "リポジトリ", "マージ"];

/// 前後から取り除く対の引用符
const QUOTE_PAIRS: [(char, char); 6] = [
    ('「', '」'),
    ('『', '』'),
    ('"', '"'),
    ('\'', '\''),
    ('“', '”'),
    ('`', '`'),
];

/// 検証を通った出力。`rescued` は、複数行の出力から最初の空でない 1 行を採ったことを表す
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    pub text: String,
    /// 複数行の出力から救済した（1 行の出力をそのまま採った場合は `false`）
    pub rescued: bool,
}

/// モデルの生の出力を、保存欄に出せる 1 文に整える。条件を満たさなければ `None`。
///
/// 戻り値の文だけが欲しい場合は `clean_summary` を使う。
pub fn clean_summary(raw: &str) -> Option<String> {
    clean_summary_detail(raw).map(|c| c.text)
}

/// `clean_summary` と同じ検証を行い、救済を使ったかも返す。
///
/// - 出力が 1 行（末尾の改行を除く）なら、その行を検証する
/// - 複数行の出力は、最初の空でない 1 行だけを検証する（救済。その行が通らなければ `None`）。
///   小型モデルが「ファイルごとに 1 行」で返す出力を、検証で全件落とさないための規則（設計 10.4）
pub fn clean_summary_detail(raw: &str) -> Option<Cleaned> {
    let trimmed = raw.trim();
    if trimmed.contains('\n') || trimmed.contains('\r') {
        let first = trimmed
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())?;
        return validate_line(first).map(|text| Cleaned {
            text,
            rescued: true,
        });
    }
    validate_line(trimmed).map(|text| Cleaned {
        text,
        rescued: false,
    })
}

/// 1 行の出力を検証する。条件（前後の整理・文の数・長さ・Git 用語）を満たさなければ `None`。
///
/// - 前後の空白・引用符・箇条書き記号を取り除く
/// - 「。」を 2 つ以上含むもの（複数文）、空、100 文字超は `None`
/// - Git 用語を含むものは `None`（英字は大文字小文字を区別しない）
/// - 日本語と数字の間の半角スペースは詰める（「第 3 章」→「第3章」）
fn validate_line(line: &str) -> Option<String> {
    let s = strip_bullet(line.trim());
    let s = strip_quotes(s.trim());
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.matches(SENTENCE_END).count() >= 2 {
        return None;
    }
    let s = close_japanese_digit_gaps(s);
    if s.chars().count() > MAX_CHARS {
        return None;
    }
    if contains_git_term(&s) {
        return None;
    }
    Some(s)
}

/// 先頭の箇条書き記号を取り除く。`-` と `*` は直後に空白がある場合だけ対象にする。
fn strip_bullet(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        let Some(first) = rest.chars().next() else {
            return rest;
        };
        let after = &rest[first.len_utf8()..];
        let is_bullet = match first {
            '-' | '*' => after.is_empty() || after.starts_with(char::is_whitespace),
            '・' | '•' | '＊' => true,
            _ => false,
        };
        if !is_bullet {
            return rest;
        }
        rest = after.trim_start();
    }
}

/// 前後が対になった引用符を、取れなくなるまで取り除く。
fn strip_quotes(s: &str) -> &str {
    let mut rest = s;
    loop {
        let mut chars = rest.chars();
        let (Some(open), Some(close)) = (chars.next(), chars.next_back()) else {
            return rest;
        };
        let paired = QUOTE_PAIRS.iter().any(|&(o, c)| o == open && c == close);
        if !paired || rest.chars().count() < 2 {
            return rest;
        }
        rest = rest[open.len_utf8()..rest.len() - close.len_utf8()].trim();
    }
}

/// 日本語と数字の間の半角スペースを詰める。日本語どうしや英字との間は変えない。
fn close_japanese_digit_gaps(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 && i + 1 < chars.len() {
            let prev = chars[i - 1];
            let next = chars[i + 1];
            let japanese_digit = is_japanese(prev) && next.is_ascii_digit();
            let digit_japanese = prev.is_ascii_digit() && is_japanese(next);
            if japanese_digit || digit_japanese {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// 日本語の文字（ひらがな、カタカナ、漢字、日本語の記号、全角形）か。
fn is_japanese(c: char) -> bool {
    matches!(
        c,
        '\u{3000}'..='\u{303F}'   // CJK の記号と句読点
        | '\u{3040}'..='\u{309F}' // ひらがな
        | '\u{30A0}'..='\u{30FF}' // カタカナ
        | '\u{3400}'..='\u{4DBF}' // 漢字（拡張 A）
        | '\u{4E00}'..='\u{9FFF}' // 漢字
        | '\u{FF01}'..='\u{FF60}' // 全角英数と記号
        | '\u{FF66}'..='\u{FF9F}' // 半角カタカナ
    )
}

/// Git 用語を含むか。英字の用語は、英数字の続きの途中での一致を除外しない（単語の先頭で一致を見る）。
fn contains_git_term(s: &str) -> bool {
    let lower = s.to_lowercase();
    let ascii = GIT_TERMS_ASCII.iter().any(|term| {
        lower.match_indices(term).any(|(idx, _)| {
            lower[..idx]
                .chars()
                .next_back()
                .is_none_or(|prev| !prev.is_ascii_alphanumeric())
        })
    });
    ascii || GIT_TERMS_JA.iter().any(|term| s.contains(term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_good_single_sentences_from_evaluation() {
        // 評価レポートの出力例（Git 用語を含まないもの）
        let good = [
            "第3章_実験.docxに温度を30度にした追加実験と、その結果を記載しました。",
            "企画書.docx の予算と実施期間を変更しました。",
            "家計簿.xlsxに旅行費用シートが追加され、合計セルが変更された。",
            "発表資料_仮.pptxを発表資料_最終版.pptxに名前変更しました。",
            "古い資料フォルダ内の会議メモと見積書の 2 つのファイルを削除しました。",
        ];
        for g in good {
            let expected = close_japanese_digit_gaps(g);
            assert_eq!(clean_summary(g).as_deref(), Some(expected.as_str()), "{g}");
        }
    }

    #[test]
    fn rejects_over_100_chars() {
        let long = "あ".repeat(101);
        assert_eq!(clean_summary(&long), None);
        let exact = "あ".repeat(100);
        assert_eq!(clean_summary(&exact).as_deref(), Some(exact.as_str()));
    }

    #[test]
    fn rejects_multi_sentence_on_one_line() {
        assert_eq!(
            clean_summary("予算を変更しました。実施期間も変更しました。"),
            None
        );
    }

    #[test]
    fn trailing_newline_is_not_multiline() {
        // 末尾の改行は取り除いたうえで 1 行として扱う（救済は使わない）
        let got = clean_summary_detail("予算を変更しました。\n").unwrap();
        assert_eq!(got.text, "予算を変更しました。");
        assert!(!got.rescued);
    }

    #[test]
    fn multiline_rescues_the_first_non_empty_line() {
        let got = clean_summary_detail("\n  \n報告書に考察を追加した。\n表も追加した。").unwrap();
        assert_eq!(got.text, "報告書に考察を追加した。");
        assert!(got.rescued);
        // 行の前後の空白も取り除く
        let got = clean_summary_detail("  予算を変更しました。  \r\n次の行").unwrap();
        assert_eq!(got.text, "予算を変更しました。");
        assert!(got.rescued);
        assert_eq!(
            clean_summary("報告書に考察を追加した。\n表も追加した。").as_deref(),
            Some("報告書に考察を追加した。")
        );
    }

    #[test]
    fn multiline_without_a_valid_first_line_is_none() {
        // 最初の行が複数文なら、後ろの行が正しくても採らない
        assert_eq!(
            clean_summary_detail("予算を変更しました。実施期間も変更しました。\n表を追加した。"),
            None
        );
        // 最初の行が Git 用語を含むなら採らない
        assert_eq!(
            clean_summary_detail("README を commit しました。\n表を追加した。"),
            None
        );
        // 空の行だけなら採らない
        assert_eq!(clean_summary_detail("\n \n"), None);
        // 最初の行が空でも、次の空でない行を救済の対象にする
        let got = clean_summary_detail("\n\n表を追加した。\n次の行").unwrap();
        assert_eq!(got.text, "表を追加した。");
        assert!(got.rescued);
    }

    #[test]
    fn rescue_is_not_used_for_single_line_outputs() {
        let got = clean_summary_detail("予算を変更しました。").unwrap();
        assert!(!got.rescued);
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(clean_summary(""), None);
        assert_eq!(clean_summary("   \n "), None);
        assert_eq!(clean_summary("「」"), None);
        assert_eq!(clean_summary("- "), None);
    }

    #[test]
    fn rejects_git_terms_case_insensitively() {
        for raw in [
            "Commit を追加しました。",
            "変更の DIFF を確認しました。",
            "README を push しました。",
            "branch を作成しました。",
            "Repository の設定を更新しました。",
            "ファイルをマージしました。",
            "コミットを作成しました。",
            "ブランチを切り替えました。",
            "プッシュしました。",
            "リポジトリを更新しました。",
        ] {
            assert_eq!(clean_summary(raw), None, "{raw}");
        }
    }

    #[test]
    fn does_not_reject_words_that_only_contain_term_in_the_middle() {
        // 単語の途中の一致（例: "undiff"）は用語とみなさない
        assert_eq!(
            clean_summary("予算を undiff の値に変更しました。").as_deref(),
            Some("予算を undiff の値に変更しました。")
        );
    }

    #[test]
    fn strips_quotes_and_bullets() {
        assert_eq!(
            clean_summary("「予算を変更しました。」").as_deref(),
            Some("予算を変更しました。")
        );
        assert_eq!(
            clean_summary("\"README を更新\"").as_deref(),
            Some("README を更新")
        );
        assert_eq!(
            clean_summary("- 予算を変更しました。").as_deref(),
            Some("予算を変更しました。")
        );
        assert_eq!(
            clean_summary("・予算を変更しました。").as_deref(),
            Some("予算を変更しました。")
        );
        // 引用符の対になっていないものは残す
        assert_eq!(
            clean_summary("「予算を変更しました。").as_deref(),
            Some("「予算を変更しました。")
        );
    }

    #[test]
    fn closes_gap_between_japanese_and_digits() {
        assert_eq!(
            clean_summary("第 3 章の実験を 5 回に修正しました。").as_deref(),
            Some("第3章の実験を5回に修正しました。")
        );
        assert_eq!(
            clean_summary("温度を 30 度に上げました。").as_deref(),
            Some("温度を30度に上げました。")
        );
    }

    #[test]
    fn keeps_spaces_between_ascii_words_and_japanese() {
        assert_eq!(
            clean_summary("README.md の使い方を追加しました。").as_deref(),
            Some("README.md の使い方を追加しました。")
        );
        // 英字と数字の間は変えない。数字と日本語の間は詰める（仕様どおり）
        assert_eq!(
            clean_summary("sales 2024 を更新しました。").as_deref(),
            Some("sales 2024を更新しました。")
        );
        assert_eq!(
            clean_summary("sales 2024 year を更新しました。").as_deref(),
            Some("sales 2024 year を更新しました。")
        );
    }
}
