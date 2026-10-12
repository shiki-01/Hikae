// モデルの出力の解析と検証（設計 10.4・10.6）。条件を満たさない出力は採用せず `None` を返す（ルールベースのまま）。
// 純関数のみ。内容はログに出さない。
//
// 出力の形式: 1 行目が要約（1 文）、2 行目が空行、3 行目以降が「・」で始まる本文（任意）。
// 検証は 2 段階。`parse_memo` は形式と長さを見る。`screen_memo` は入力と照らして、
// 根拠の確認・長い写し・機微情報を見る（本文の行だけを捨て、要約が問題なら全体を採らない）。

use crate::prompt::ChangeInput;
use crate::redact::{copies_diff, looks_sensitive};

/// 要約・本文の 1 行の上限（文字数）。評価で、事実として正しい出力が 80 文字を超える例（xlsx の複数セルの変更で 87 文字）が
/// あったため 100 文字とする（プロンプトの「60 文字程度」は小型モデルが守らない）
const MAX_CHARS: usize = 100;

/// 本文の行数の上限
pub const MAX_BODY_LINES: usize = 5;
/// 本文の全体の上限（文字数。行の文字数の合計）
pub const MAX_BODY_CHARS: usize = 400;
/// 要約が差分の本文の連続した何文字以上を写したら捨てるか（空白と記号を除いて比べる）
pub const TITLE_COPY_CHARS: usize = 24;
/// 本文の行が差分の本文の連続した何文字以上を写したら捨てるか（空白と記号を除いて比べる）
pub const BODY_COPY_CHARS: usize = 12;

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

/// 解析・検証を通ったメモの提案（要約と本文）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Memo {
    /// 要約（1 行。1 文）
    pub title: String,
    /// 本文の行（「・」の記号は外してある）。無ければ空
    pub body: Vec<String>,
    /// 安全のために本文の行を捨てたか（形式・長さ・根拠・機微・写しのいずれか）
    pub body_dropped: bool,
}

/// モデルの生の出力を、形式どおりに解析して検証する（入力とは照らさない）。
///
/// - 要約: 最初の空でない行。前置きなら `None`。箇条書き記号と引用符を外し、1 文・100 文字以内・Git 用語なし
/// - 本文: 要約の直後が空行のときは、その後の「・」「-」「*」で始まる行だけ。空行が無いとき（旧形式の複数行）は、
///   残りの行をすべて本文の候補にする。各行は要約と同じ検証。落ちた行は捨てて `body_dropped` を立てる
/// - 本文は最大 `MAX_BODY_LINES` 行・全体 `MAX_BODY_CHARS` 文字まで（超えた行は捨てる）
pub fn parse_memo(raw: &str) -> Option<Memo> {
    let lines: Vec<&str> = raw.lines().collect();
    let first_idx = lines.iter().position(|line| !line.trim().is_empty())?;
    let first = lines[first_idx].trim();
    // 前置きの判定は、複数行の出力のときだけ語で見る（1 行の出力は、旧規則どおり語では判定しない）。
    // 末尾の「:」は、1 行でも要約ではないため常に前置きとする
    let multi_line = lines.iter().filter(|line| !line.trim().is_empty()).count() > 1;
    if ends_with_colon(first) || (multi_line && is_preamble(first)) {
        return None;
    }
    let title = validate_line(first)?;

    let rest = &lines[first_idx + 1..];
    let blank_after_title = rest.first().is_some_and(|line| line.trim().is_empty());
    let mut body = Vec::new();
    let mut total_chars = 0usize;
    let mut dropped = false;
    for line in rest
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
    {
        // 空行の後の本文は箇条書きだけを読む（それ以外の行は本文ではないため、捨てるが落とした扱いにはしない）
        if blank_after_title && !starts_with_bullet(line) {
            continue;
        }
        let Some(text) = body_line(line) else {
            dropped = true;
            continue;
        };
        let chars = text.chars().count();
        if body.len() >= MAX_BODY_LINES || total_chars + chars > MAX_BODY_CHARS {
            dropped = true;
            continue;
        }
        total_chars += chars;
        body.push(text);
    }
    Some(Memo {
        title,
        body,
        body_dropped: dropped,
    })
}

/// 本文の 1 行を検証する。前置き（末尾が `:` の行など）も本文として採らない。
fn body_line(line: &str) -> Option<String> {
    if is_preamble(line) {
        return None;
    }
    validate_line(line)
}

/// 本文の行の先頭が箇条書きの記号か（「・」「-」「*」）
fn starts_with_bullet(line: &str) -> bool {
    line.starts_with(['・', '-', '*'])
}

/// 解析済みの提案を、入力と照らして検証する（設計 10.4 の根拠の確認、10.6 の機微情報と長い写し）。
///
/// - 要約が機微に見える・差分の長い写し（`TITLE_COPY_CHARS` 以上）を含むなら `None`（提案全体を採らない）
/// - 本文の行は、機微に見える・長い写し（`BODY_COPY_CHARS` 以上）・根拠が無い（後述）のいずれかなら捨てる
///   （`body_dropped` を立てる）
/// - 根拠: 行が、変更されたファイルの名前（拡張子を除いた名前を含む）か、差分の本文の語（日本語は 2 文字以上、
///   英数字は 3 文字以上のかたまり）のどれかを含むこと
pub fn screen_memo(mut memo: Memo, input: &ChangeInput) -> Option<Memo> {
    let corpus = diff_corpus(input);
    if looks_sensitive(&memo.title).is_some() || copies_diff(&memo.title, &corpus, TITLE_COPY_CHARS)
    {
        return None;
    }
    let names = file_names(input);
    let words = grounding_words(&corpus);
    let mut kept = Vec::with_capacity(memo.body.len());
    for line in std::mem::take(&mut memo.body) {
        let ok = looks_sensitive(&line).is_none()
            && !copies_diff(&line, &corpus, BODY_COPY_CHARS)
            && grounded(&line, &names, &words);
        if ok {
            kept.push(line);
        } else {
            memo.body_dropped = true;
        }
    }
    memo.body = kept;
    Some(memo)
}

/// キャッシュに保存する文字列（要約・空行・本文を改行でつないだもの）。本文が無ければ要約だけ。
pub fn memo_text(memo: &Memo) -> String {
    if memo.body.is_empty() {
        memo.title.clone()
    } else {
        format!("{}\n\n{}", memo.title, memo.body.join("\n"))
    }
}

/// `memo_text` で作った文字列から提案に戻す。要約が空なら `None`。
pub fn memo_from_text(text: &str) -> Option<Memo> {
    let (title, body) = match text.split_once("\n\n") {
        Some((title, body)) => (title, body),
        None => (text, ""),
    };
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let body = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    Some(Memo {
        title: title.to_string(),
        body,
        body_dropped: false,
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

/// 出力の行が前置き（本文の前置きの言い回し）か。純関数。
///
/// - 末尾が `:` または `：` の行（「次の内容です:」など）
/// - 先頭が `PREAMBLE_PREFIXES` のいずれかで、短い行（`PREAMBLE_MAX_CHARS` 未満）の行
///
/// 箇条書き記号と対の引用符は外して判定する。長い行は、同じ語で始まっても前置きとみなさない
/// （「メモリの設定を…」のような本文を誤って弾かないため）。
fn is_preamble(line: &str) -> bool {
    let probe = strip_quotes(strip_bullet(line.trim()).trim()).trim();
    if ends_with_colon(probe) {
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

/// 末尾が `:` または `：` の行か（「次の内容です:」など。導入の言い回しとみなす）
fn ends_with_colon(line: &str) -> bool {
    let probe = line.trim();
    probe.ends_with(':') || probe.ends_with('：')
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

/// 差分の本文を 1 つにまとめたもの（写しの検査と根拠の確認に使う）
fn diff_corpus(input: &ChangeInput) -> String {
    input
        .files
        .iter()
        .filter_map(|file| file.diff_text.as_deref())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 根拠に使うファイル名（名前・旧名と、拡張子を除いた名前）。2 文字未満は、どの行にも含まれて根拠にならないため除く
fn file_names(input: &ChangeInput) -> Vec<String> {
    let mut out = Vec::new();
    for file in &input.files {
        for name in std::iter::once(file.name.as_str()).chain(file.old_name.as_deref()) {
            if name.chars().count() >= 2 {
                out.push(name.to_string());
            }
            if let Some(idx) = name.rfind('.') {
                let stem = &name[..idx];
                if stem.chars().count() >= 2 {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out
}

/// 行が根拠を持つか（ファイル名、または差分の本文の語を含む）
fn grounded(line: &str, names: &[String], words: &[String]) -> bool {
    names.iter().any(|name| line.contains(name.as_str()))
        || words.iter().any(|word| line.contains(word.as_str()))
}

/// 差分の本文の語。日本語（ひらがな・カタカナ・漢字）は 2 文字以上、英数字は 3 文字以上のかたまり。
fn grounding_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut run = String::new();
    let mut run_is_ja = false;
    for c in text.chars().chain(std::iter::once('\n')) {
        let class = if is_word_ja(c) {
            Some(true)
        } else if c.is_ascii_alphanumeric() {
            Some(false)
        } else {
            None
        };
        match class {
            Some(ja) if !run.is_empty() && ja == run_is_ja => run.push(c),
            Some(ja) => {
                take_word(&mut run, run_is_ja, &mut words);
                run.push(c);
                run_is_ja = ja;
            }
            None => take_word(&mut run, run_is_ja, &mut words),
        }
    }
    words
}

/// 連続する文字（かたまり）から語を取り出す（`run` は空にする）。
///
/// - 英数字は、かたまりが 3 文字以上なら、そのかたまり全体を語にする
/// - 日本語は、かたまりの中の 2 文字の並びを語にする。かたまりは助詞（「は」「の」など）を含んで長くなるため、
///   かたまり全体を語にすると、差分と同じ語順で書かれた行しか通らない（設計 10.4 の根拠の確認の意図から外れる）
fn take_word(run: &mut String, is_ja: bool, words: &mut Vec<String>) {
    let chars: Vec<char> = std::mem::take(run).chars().collect();
    if is_ja {
        words.extend(chars.windows(2).map(|pair| pair.iter().collect::<String>()));
    } else if chars.len() >= 3 {
        words.push(chars.iter().collect());
    }
}

/// 語を作る日本語の文字（ひらがな・カタカナ・漢字）。記号と全角形は含めない
fn is_word_ja(c: char) -> bool {
    matches!(
        c,
        '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{ChangeKind, ChangedFile};

    fn file(name: &str, diff: Option<&str>) -> ChangedFile {
        ChangedFile {
            kind: ChangeKind::Modified,
            name: name.into(),
            old_name: None,
            amount: None,
            diff_text: diff.map(str::to_string),
            sensitive: false,
        }
    }

    fn input(files: Vec<ChangedFile>) -> ChangeInput {
        ChangeInput { files }
    }

    #[test]
    fn summary_only_output_is_accepted() {
        let memo = parse_memo("契約書の支払期限を翌々月払いに変えた。").expect("要約");
        assert_eq!(memo.title, "契約書の支払期限を翌々月払いに変えた。");
        assert!(memo.body.is_empty());
        assert!(!memo.body_dropped);
    }

    #[test]
    fn summary_and_bullets_are_split() {
        let raw = "在庫を見直した。\n\n・商品Aを40個から25個に変更\n・商品Cを10個から0個に変更";
        let memo = parse_memo(raw).expect("解析");
        assert_eq!(memo.title, "在庫を見直した。");
        assert_eq!(
            memo.body,
            vec!["商品Aを40個から25個に変更", "商品Cを10個から0個に変更"]
        );
        assert!(!memo.body_dropped);
    }

    #[test]
    fn bullet_marks_are_removed_in_all_forms() {
        let raw = "要約。\n\n- 一行目の変更\n* 二行目の変更\n・三行目の変更";
        let memo = parse_memo(raw).expect("解析");
        assert_eq!(
            memo.body,
            vec!["一行目の変更", "二行目の変更", "三行目の変更"]
        );
    }

    #[test]
    fn lines_without_a_bullet_after_the_blank_line_are_ignored() {
        let raw = "要約。\n\n以上です\n・本文の一行";
        let memo = parse_memo(raw).expect("解析");
        assert_eq!(memo.body, vec!["本文の一行"]);
        assert!(!memo.body_dropped);
    }

    #[test]
    fn old_multi_line_format_uses_the_rest_as_body_candidates() {
        // 空行が無い（旧形式）: 最初の行が要約、残りが本文の候補
        let raw = "報告書の数字を更新した\n表 1 の金額を変更\n表 2 の注釈を追加";
        let memo = parse_memo(raw).expect("解析");
        assert_eq!(memo.title, "報告書の数字を更新した");
        assert_eq!(memo.body, vec!["表1の金額を変更", "表2の注釈を追加"]);
    }

    #[test]
    fn preamble_as_the_first_line_is_rejected() {
        assert_eq!(parse_memo("以下が保存メモです。\n\n・変更"), None);
        assert_eq!(parse_memo("次の内容です:\n変更"), None);
        assert_eq!(parse_memo("Here is the memo.\n変更"), None);
        // 長い行は、同じ語で始まっても前置きとみなさない
        // 1 行の出力は語では判定しない（旧規則）。末尾の「:」は 1 行でも前置き
        assert!(parse_memo("保存メモです。").is_some());
        assert_eq!(parse_memo("次の内容です:"), None);
        // 前置きの判定は 40 文字未満の行だけが対象（既存の規則）
        assert!(parse_memo("メモリの設定を見直し、起動時の確保量を減らして、再起動後の動作も確かめ、結果を記録した。").is_some());
    }

    #[test]
    fn empty_and_invalid_summaries_are_rejected() {
        assert_eq!(parse_memo(""), None);
        assert_eq!(parse_memo("   \n\n"), None);
        // 2 文
        assert_eq!(parse_memo("一つ目。二つ目。"), None);
        // Git 用語
        assert_eq!(parse_memo("commit の内容を直した"), None);
        assert_eq!(parse_memo("コミットを直した"), None);
        // 100 文字超
        let long = "あ".repeat(101);
        assert_eq!(parse_memo(&long), None);
        assert!(parse_memo(&"あ".repeat(100)).is_some());
    }

    #[test]
    fn invalid_body_lines_are_dropped_and_flagged() {
        let raw = "要約。\n\n・良い行\n・commit の行\n・二つ。目。";
        let memo = parse_memo(raw).expect("要約は採る");
        assert_eq!(memo.body, vec!["良い行"]);
        assert!(memo.body_dropped);
    }

    #[test]
    fn body_has_at_most_five_lines_and_400_chars() {
        let six: String = (1..=6).map(|i| format!("・行{i}の変更\n")).collect();
        let raw = format!("要約。\n\n{six}");
        let memo = parse_memo(&raw).expect("解析");
        assert_eq!(memo.body.len(), MAX_BODY_LINES);
        assert!(memo.body_dropped);

        let big: String = (0..5).map(|_| format!("・{}\n", "あ".repeat(90))).collect();
        let memo = parse_memo(&format!("要約。\n\n{big}")).expect("解析");
        // 90 文字 × 4 行 = 360 文字まで。5 行目（450 文字目）で全体の上限を超えるため捨てる
        assert_eq!(memo.body.len(), 4);
        assert!(memo.body_dropped);
    }

    #[test]
    fn japanese_digit_gaps_are_closed_in_summary_and_body() {
        let memo = parse_memo("第 3 章を更新。\n\n・表 2 を直す").expect("解析");
        assert_eq!(memo.title, "第3章を更新。");
        assert_eq!(memo.body, vec!["表2を直す"]);
    }

    #[test]
    fn grounding_accepts_file_names_and_stems_and_diff_words() {
        let inp = input(vec![
            file("契約書ひな形.docx", Some("+ 支払は翌々月払いとする")),
            file("在庫表.xlsx", Some("+ Sheet1!B4: 120000 → 135000")),
        ]);
        let memo = Memo {
            title: "要約".into(),
            body: vec![
                "契約書ひな形の内容を直した".into(), // ファイル名（拡張子なし）
                "在庫表に変更を加えた".into(),       // 拡張子なしの名前
                "支払の条件を変えた".into(),         // 差分の語「支払」
                "Sheet1の値を増やした".into(),       // 差分の英数字の語
                "まったく関係のない文章です".into(), // 根拠なし
            ],
            body_dropped: false,
        };
        let screened = screen_memo(memo, &inp).expect("要約は通る");
        assert_eq!(
            screened.body,
            vec![
                "契約書ひな形の内容を直した",
                "在庫表に変更を加えた",
                "支払の条件を変えた",
                "Sheet1の値を増やした",
            ]
        );
        assert!(screened.body_dropped);
    }

    #[test]
    fn grounding_words_follow_the_length_rules() {
        let words = grounding_words("+ 支 ab 税率10% Sheet1!B4 xyz");
        // 日本語は 2 文字以上（「支」は 1 文字なので除く）。英数字は 3 文字以上（ab は除く）
        assert!(words.contains(&"税率".to_string()));
        assert!(words.contains(&"Sheet1".to_string()));
        assert!(words.contains(&"xyz".to_string()));
        assert!(!words.contains(&"支".to_string()));
        assert!(!words.contains(&"ab".to_string()));
    }

    #[test]
    fn a_summary_that_copies_the_diff_is_refused() {
        let diff = "+ 違約金は未払い額の一割とし、支払期日から三十日を経過した時点で発生する";
        let inp = input(vec![file("契約.txt", Some(diff))]);
        // 24 文字以上の写し: 要約として採らない
        let copied = Memo {
            title: "違約金は未払い額の一割とし、支払期日から三十日を経過した".into(),
            body: vec![],
            body_dropped: false,
        };
        assert_eq!(screen_memo(copied, &inp), None);
        // 写しでなければ採る
        let own = Memo {
            title: "契約の違約金の定めを厳しくした".into(),
            body: vec![],
            body_dropped: false,
        };
        assert!(screen_memo(own, &inp).is_some());
    }

    #[test]
    fn a_body_line_that_copies_the_diff_is_dropped() {
        let diff = "+ 違約金は未払い額の一割とする";
        let inp = input(vec![file("契約.txt", Some(diff))]);
        let memo = Memo {
            title: "契約の違約金を変えた".into(),
            body: vec![
                "違約金は未払い額の一割とする".into(),
                "違約金の定めを厳しくした".into(),
            ],
            body_dropped: false,
        };
        let screened = screen_memo(memo, &inp).expect("要約は通る");
        assert_eq!(screened.body, vec!["違約金の定めを厳しくした"]);
        assert!(screened.body_dropped);
    }

    #[test]
    fn sensitive_text_in_the_summary_refuses_the_whole_suggestion() {
        let inp = input(vec![file("設定.txt", Some("+ x"))]);
        let memo = Memo {
            title: "接続先の認証情報を更新: password=hunter2".into(),
            body: vec![],
            body_dropped: false,
        };
        assert_eq!(screen_memo(memo, &inp), None);
    }

    #[test]
    fn sensitive_body_lines_are_dropped() {
        let inp = input(vec![file("設定.txt", Some("+ 接続の設定を変えた"))]);
        let memo = Memo {
            title: "接続の設定を変えた".into(),
            body: vec![
                "接続の設定を変えた。連絡先 test@example.com".into(),
                "接続の設定を変えた".into(),
            ],
            body_dropped: false,
        };
        let screened = screen_memo(memo, &inp).expect("要約は通る");
        assert_eq!(screened.body, vec!["接続の設定を変えた"]);
        assert!(screened.body_dropped);
    }

    #[test]
    fn memo_text_round_trips_through_the_cache_format() {
        let memo = Memo {
            title: "要約".into(),
            body: vec!["一つ目".into(), "二つ目".into()],
            body_dropped: true,
        };
        let text = memo_text(&memo);
        assert_eq!(text, "要約\n\n一つ目\n二つ目");
        let back = memo_from_text(&text).expect("復元");
        assert_eq!(back.title, "要約");
        assert_eq!(back.body, vec!["一つ目", "二つ目"]);
        assert!(!back.body_dropped);
        // 本文が無いものは要約だけ
        let only = Memo {
            title: "要約だけ".into(),
            body: vec![],
            body_dropped: false,
        };
        assert_eq!(memo_text(&only), "要約だけ");
        assert_eq!(
            memo_from_text("要約だけ").expect("復元").body,
            Vec::<String>::new()
        );
    }

    #[test]
    fn strip_bullet_and_quotes_follow_the_old_rules() {
        assert_eq!(strip_bullet("- x"), "x");
        assert_eq!(strip_bullet("-x"), "-x");
        assert_eq!(strip_quotes("「内容」"), "内容");
        assert_eq!(strip_quotes("『内容』"), "内容");
    }
}
