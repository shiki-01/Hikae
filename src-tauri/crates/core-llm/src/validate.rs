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

/// 前置きと判断する行の長さの上限（文字数。これ未満の行で、先頭が `PREAMBLE_PREFIXES` のものが前置き）
const PREAMBLE_MAX_CHARS: usize = 40;

/// 前置きの先頭語（英字は大文字小文字を区別しない）。小型モデルが本文の前に付ける定型の言い回し
const PREAMBLE_PREFIXES: [&str; 7] = ["以下", "次の", "保存メモ", "メモ", "Here", "Sure", "回答"];

/// 検証を通った出力。`rescued` は、複数行の出力を 1 文に整えて（行をつなぐか、最初の 1 行を採って）採ったことを表す
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
/// - 複数行の出力は、2〜4 行なら「、」でつないで 1 文にして検証する。つなぐと検証を通らない（長すぎる等）
///   ときや行数が多いときは、最初の空でない 1 行だけを検証する（救済。その行も通らなければ `None`）。
///   小型モデルが「ファイルごとに 1 行」で返す出力を、検証で全件落とさないための規則（設計 10.4）。
///   ただし最初の行が前置き（`is_preamble`）なら、本文ではないため採らない
/// - 1 行の出力（改行なし）には、前置きの判定を適用しない
pub fn clean_summary_detail(raw: &str) -> Option<Cleaned> {
    let trimmed = raw.trim();
    if trimmed.contains('\n') || trimmed.contains('\r') {
        let lines: Vec<&str> = trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let first = *lines.first()?;
        if is_preamble(first) {
            return None;
        }
        // 複数ファイルの変更で「ファイルごとに 1 行」と返ったとき、最初の 1 行だけでは他のファイルの内容が
        // 落ちる。行を「、」でつないで 1 文にし、検証を通ればそれを採る。通らなければ最初の 1 行だけを採る
        if let Some(text) =
            join_lines(&lines).and_then(|joined| validate_line_max(&joined, JOIN_MAX_CHARS))
        {
            return Some(Cleaned {
                text,
                rescued: true,
            });
        }
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

/// つなぐ行数の上限（これを超える出力は、つながずに最初の 1 行だけを採る）
const JOIN_MAX_LINES: usize = 4;

/// つないだ出力の長さの上限（文字数）。複数ファイルの変更を 1 文にまとめると、1 ファイル 1 行の
/// 出力を足して 100 文字を超えやすい（実測: 5 ファイルの混在で 4 行・約 130 文字）。
/// 1 行の出力の上限（`MAX_CHARS`）より長くしてよいのは、行ごとの内容が落ちないことを優先するため
const JOIN_MAX_CHARS: usize = 160;

/// 複数行の出力を、行ごとに整えて「、」でつなぎ、末尾を 1 つの「。」で終える。
/// 2〜`JOIN_MAX_LINES` 行のときだけつなぐ（それ以外は `None`）。途中に前置きの行があれば `None`。
fn join_lines(lines: &[&str]) -> Option<String> {
    if lines.len() < 2 || lines.len() > JOIN_MAX_LINES {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(lines.len());
    for line in lines {
        if is_preamble(line) {
            return None;
        }
        let s = strip_bullet(line.trim());
        let s = strip_quotes(s.trim()).trim();
        let s = s.trim_end_matches(['。', '.', '、', ',']).trim();
        if s.is_empty() {
            return None;
        }
        parts.push(s.to_string());
    }
    Some(format!("{}。", parts.join("、")))
}

/// 1 行の出力を検証する。条件（前後の整理・文の数・長さ・Git 用語）を満たさなければ `None`。
///
/// - 前後の空白・引用符・箇条書き記号を取り除く
/// - 「。」を 2 つ以上含むもの（複数文）、空、100 文字超は `None`
/// - Git 用語を含むものは `None`（英字は大文字小文字を区別しない）
/// - 日本語と数字の間の半角スペースは詰める（「第 3 章」→「第3章」）
fn validate_line(line: &str) -> Option<String> {
    validate_line_max(line, MAX_CHARS)
}

/// `validate_line` の、長さの上限（文字数）を指定する版
fn validate_line_max(line: &str, max_chars: usize) -> Option<String> {
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
    if s.chars().count() > max_chars {
        return None;
    }
    if contains_git_term(&s) {
        return None;
    }
    Some(s)
}

/// 複数行の出力の 1 行目が前置き（本文の前置きの言い回し）か。純関数。
///
/// - 末尾が `:` または `：` の行（「次の内容です:」など）
/// - 先頭が `PREAMBLE_PREFIXES` のいずれかで、短い行（`PREAMBLE_MAX_CHARS` 未満）の行
///
/// 箇条書き記号と対の引用符は外して判定する。長い行は、同じ語で始まっても前置きとみなさない
/// （「メモリの設定を…」のような本文を誤って弾かないため）。
fn is_preamble(line: &str) -> bool {
    let probe = strip_quotes(strip_bullet(line.trim()).trim()).trim();
    if probe.ends_with(':') || probe.ends_with('：') {
        return true;
    }
    if probe.chars().count() >= PREAMBLE_MAX_CHARS {
        return false;
    }
    let lower = probe.to_lowercase();
    PREAMBLE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(&prefix.to_lowercase()))
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
    fn multiline_lines_are_joined_into_one_sentence() {
        // ファイルごとに 1 行で返った出力は、行を「、」でつないで 1 文にする（他のファイルの内容を落とさない）
        let got = clean_summary_detail("\n  \n報告書に考察を追加した。\n表も追加した。").unwrap();
        assert_eq!(got.text, "報告書に考察を追加した、表も追加した。");
        assert!(got.rescued);
        // 箇条書き記号・行の前後の空白・末尾の句点はそろえてつなぐ
        let got = clean_summary_detail("- 予算を150万円に変更  \r\n・期間を5月からにした").unwrap();
        assert_eq!(got.text, "予算を150万円に変更、期間を5月からにした。");
        assert!(got.rescued);
        assert_eq!(
            clean_summary("報告書に考察を追加した。\n表も追加した。\n図を差し替えた。").as_deref(),
            Some("報告書に考察を追加した、表も追加した、図を差し替えた。")
        );
    }

    #[test]
    fn multiline_falls_back_to_the_first_line_when_joining_does_not_work() {
        // 前置きに当たる行が混ざる（前置きの判定は短い行の先頭語）ときは、最初の 1 行だけを採る
        let got = clean_summary_detail("  予算を変更しました。  \r\n次の行").unwrap();
        assert_eq!(got.text, "予算を変更しました。");
        assert!(got.rescued);
        // つなぐと 100 文字を超えても、160 文字以内ならつなぐ（行ごとの内容を落とさない）
        let mid = format!("{}。\n{}。", "あ".repeat(60), "い".repeat(60));
        let got = clean_summary_detail(&mid).unwrap();
        assert_eq!(
            got.text,
            format!("{}、{}。", "あ".repeat(60), "い".repeat(60))
        );
        // つなぐと 160 文字を超えるときは、最初の 1 行だけを採る
        let long = format!("{}。\n{}。", "あ".repeat(90), "い".repeat(90));
        let got = clean_summary_detail(&long).unwrap();
        assert_eq!(got.text, format!("{}。", "あ".repeat(90)));
        // 5 行以上はつながず、最初の 1 行だけを採る
        let many = "a1を更新\nb2を更新\nc3を更新\nd4を更新\ne5を更新";
        let got = clean_summary_detail(many).unwrap();
        assert_eq!(got.text, "a1を更新");
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

    #[test]
    fn multiline_first_line_that_is_a_preamble_is_not_rescued() {
        // 前置き（末尾がコロン）の次の行が正しくても、前置きの行は採らない
        assert_eq!(
            clean_summary_detail("以下の内容です:\n予算を変更しました。"),
            None
        );
        assert_eq!(
            clean_summary_detail("変更は次のとおりです：\n予算を変更しました。"),
            None
        );
        // 先頭の語が前置きの言い回し（短い行）
        for head in [
            "以下の通りです\n予算を変更しました。",
            "次の文です\n予算を変更しました。",
            "保存メモ\n予算を変更しました。",
            "メモ: 予算を変更しました。\n表も追加した。",
            "回答\n予算を変更しました。",
            "Here is the memo\n予算を変更しました。",
            "Sure, here you go\n予算を変更しました。",
            "「以下の通りです」\n予算を変更しました。",
            "- 以下の通りです\n予算を変更しました。",
        ] {
            assert_eq!(clean_summary_detail(head), None, "{head}");
        }
        // 大文字小文字は区別しない
        assert_eq!(
            clean_summary_detail("here is the memo\n予算を変更しました。"),
            None
        );
    }

    #[test]
    fn multiline_first_line_that_is_content_is_still_rescued() {
        // 前置きの語を含んでいても、先頭でなければ本文
        let got = clean_summary_detail("報告書に保存メモを追加した。\n表も追加した。").unwrap();
        assert_eq!(got.text, "報告書に保存メモを追加した、表も追加した。");
        assert!(got.rescued);
        // 前置きの語で始まっても、長い行は本文として採る（40 文字以上）
        let long = format!("メモ{}。\n次の行", "あ".repeat(40));
        let got = clean_summary_detail(&long).unwrap();
        assert_eq!(got.text, format!("メモ{}。", "あ".repeat(40)));
        assert!(got.rescued);
    }

    #[test]
    fn preamble_rule_does_not_apply_to_single_line_outputs() {
        // 改行を含まない出力は、前置きの判定を行わない（通常の 1 行は弾かない）
        let got = clean_summary_detail("以下の内容です:").unwrap();
        assert_eq!(got.text, "以下の内容です:");
        assert!(!got.rescued);
        assert_eq!(
            clean_summary("回答: 予算を変更しました。").as_deref(),
            Some("回答: 予算を変更しました。")
        );
    }
}
