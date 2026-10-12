// 機微情報の対策（設計書 10.6）。純関数のみ。Tauri・git には依存しない。
//
// - 入力側（モデルへ渡す前）: `sensitive_file`（機微なファイル名は本文を渡さない）、`mask_secrets`（差分の本文と名前を伏せ字にする）
// - 出力側（モデルの出力の検査）: `looks_sensitive`（該当する行は捨てる）、`copies_diff`（差分の長い写しを捨てる）
// - 利用者の入力側（保存欄の警告）: `memo_sensitive_kinds`
//
// 検出の結果は種類（`SensitiveKind`）だけを返す。検出した文字列の内容は返さず、ログにも出さない。
// パターンはこのファイルの `rules` の 1 か所だけに置く（TypeScript に複製しない）。
// 残余リスク（ラベルの無い短い秘密、住所、氏名など）は設計書 10.6 に記載する。

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 伏せ字の表記（設計書 10.6）
pub const MASK: &str = "［伏せ字］";

/// 検出された機微情報の種類（画面の文言を選ぶために使う。内容は含まない）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SensitiveKind {
    /// 秘密鍵（PEM のブロック）
    PrivateKey,
    /// クラウドのアクセスキー（AWS など）
    AccessKey,
    /// 各サービスのトークン・API キー
    Token,
    /// JWT・Bearer のトークン
    BearerToken,
    /// URL 内の認証情報（`ユーザー:パスワード@`）
    UrlCredential,
    /// `パスワード: …` のようなラベル付きの値
    LabeledValue,
    /// メールアドレス
    Email,
    /// 電話番号
    Phone,
    /// クレジットカード番号（Luhn を満たすもの）
    CardNumber,
    /// マイナンバー相当（前後が数字でない連続 12 桁）
    LongNumber,
    /// 高エントロピーの長い文字列（英数字と記号 32 文字以上）
    HighEntropy,
    /// 伏せ字そのもの（出力に入っていたら、その行は捨てる）
    Masked,
}

/// 検出のパターン（まとめて 1 か所に置く）
struct Rules {
    /// PEM の秘密鍵ブロック。END が無ければ末尾まで伏せる
    pem: Regex,
    /// URL 内の認証情報。前後の部分（`://user:`、`@`）を残して値だけを伏せる
    url_credential: Regex,
    /// `Bearer <トークン>`。ラベルを残して値だけを伏せる
    bearer: Regex,
    /// 英字のラベル付きの値（大文字小文字を問わない）。ラベルと区切りを残して値だけを伏せる
    labeled_en: Regex,
    /// 日本語のラベル付きの値。ラベルと区切りを残して値だけを伏せる
    labeled_ja: Regex,
    /// そのまま伏せる形式の鍵（AWS、GitHub、Slack、sk- など）
    key_shapes: Vec<(SensitiveKind, Regex)>,
    /// JWT（`eyJ…` の 3 区切り）
    jwt: Regex,
    /// メールアドレス
    email: Regex,
    /// カード番号の候補（数字とハイフン・スペース区切り）。Luhn で確かめる
    card: Regex,
    /// 日本の電話番号（ハイフン区切り、市外局番から始まる）
    phone_hyphen: Regex,
    /// 日本の電話番号（ハイフンなしの 10〜11 桁）
    phone_plain: Regex,
    /// 国際表記（+81）の電話番号
    phone_intl: Regex,
    /// 連続 12 桁（マイナンバー相当）
    twelve_digits: Regex,
    /// 高エントロピーの候補（英数字と記号。パス・URL の一部は後で除く）
    entropy: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        // 正規表現は固定の文字列で、構文の誤りは開発中に必ず見つかる（テストで全件コンパイルする）
        let re = |p: &str| Regex::new(p).expect("固定の正規表現");
        Rules {
            pem: re(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)"),
            url_credential: re(r"(://[^\s/:@]+:)[^\s/@]+(@)"),
            bearer: re(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{16,}"),
            labeled_en: re(
                r#"(?i)((?:password|passwd|pwd|pass|secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|client[_-]?secret|credential|authorization|auth)["']?\s*[:=]\s*["']?)[^\s"',;]+"#,
            ),
            labeled_ja: re(
                r#"((?:パスワード|暗証番号|秘密(?:鍵|のキー)?|トークン|認証(?:キー|情報)?|ＩＤ|ID)["']?\s*[:：=＝]\s*["']?)[^\s"',;]+"#,
            ),
            key_shapes: vec![
                (SensitiveKind::AccessKey, re(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
                (SensitiveKind::Token, re(r"\bgh[pousr]_[A-Za-z0-9]{20,}")),
                (SensitiveKind::Token, re(r"\bgithub_pat_[A-Za-z0-9_]{20,}")),
                (SensitiveKind::Token, re(r"\bxox[abprs]-[A-Za-z0-9-]{10,}")),
                (
                    SensitiveKind::Token,
                    re(r"\b(?:sk-|sk_live_|pk_live_|rk_live_)[A-Za-z0-9_-]{16,}"),
                ),
            ],
            jwt: re(r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*"),
            email: re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
            card: re(r"[0-9](?:[ -]?[0-9]){12,18}"),
            phone_hyphen: re(r"0[0-9]{1,4}-[0-9]{1,4}-[0-9]{3,4}"),
            phone_plain: re(r"0[0-9]{9,10}"),
            phone_intl: re(r"\+81[- ]?[0-9]{1,4}[- ]?[0-9]{1,4}[- ]?[0-9]{3,4}"),
            twelve_digits: re(r"[0-9]{12}"),
            entropy: re(r"[A-Za-z0-9+/=_-]{32,}"),
        }
    })
}

/// 伏せ字の走査。`found` に、伏せた箇所の種類を 1 件ずつ入れる。
/// 戻り値は伏せ字にした文字列。`rewrite` が `None` を返した一致は伏せない。
fn mask_each(
    text: &str,
    re: &Regex,
    kind: SensitiveKind,
    found: &mut Vec<SensitiveKind>,
    mut rewrite: impl FnMut(&Captures) -> Option<String>,
) -> String {
    let mut hits = 0usize;
    let out = re.replace_all(text, |caps: &Captures| match rewrite(caps) {
        Some(s) => {
            hits += 1;
            s
        }
        None => caps.get(0).map_or("", |m| m.as_str()).to_string(),
    });
    found.extend(std::iter::repeat_n(kind, hits));
    out.into_owned()
}

/// 一致の一つ目以外の部分（前後の残す部分）を取り出す
fn group<'h>(caps: &Captures<'h>, index: usize) -> &'h str {
    caps.get(index).map_or("", |m| m.as_str())
}

/// 前後が ASCII の数字でないか（数字の列の途中の一致を除く）
fn digit_bounded(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(|c| c.is_ascii_digit()) && !after.is_some_and(|c| c.is_ascii_digit())
}

/// Luhn のチェックデジット（クレジットカード番号の検査）
fn luhn_ok(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// 高エントロピーの候補か。英字の大文字・小文字・数字のうち 2 種類以上を含み、
/// パス・URL・ファイル名の一部（前後が `/` `\` `:` `.`、または `/` を 2 つ以上含む）は除く
fn high_entropy_token(text: &str, start: usize, end: usize, token: &str) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    if matches!(before, Some('/' | '\\' | ':')) || matches!(after, Some('/' | '\\' | '.')) {
        return false;
    }
    // `/` を含む候補は、パスの一部とみなす（各区切りが 4 文字以上で、`-` `_` を含まないものだけを秘密の候補にする）。
    // 標準の Base64 は `-` `_` を使わないため、パスやファイル名の多くはここで除かれる
    if token.contains('/') {
        let short_part = token.split('/').any(|part| part.chars().count() < 4);
        if short_part || token.contains(['-', '_']) {
            return false;
        }
    }
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let has_upper = token.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = token.chars().any(|c| c.is_ascii_lowercase());
    [has_digit, has_upper, has_lower]
        .iter()
        .filter(|&&b| b)
        .count()
        >= 2
}

/// 機微情報を伏せ字にし、検出の種類を伏せた箇所の数だけ返す（検出と伏せ字は同じ経路）。
fn scan(text: &str) -> (String, Vec<SensitiveKind>) {
    let r = rules();
    let mut found = Vec::new();
    // 1. 秘密鍵のブロック（最初に伏せる。中の行が他のパターンに掛からないように）
    let mut out = mask_each(text, &r.pem, SensitiveKind::PrivateKey, &mut found, |_| {
        Some(MASK.to_string())
    });
    // 2. 認証情報を含む形式（ラベルや前後の部分を残し、値だけを伏せる）
    out = mask_each(
        &out,
        &r.url_credential,
        SensitiveKind::UrlCredential,
        &mut found,
        |c| Some(format!("{}{MASK}{}", group(c, 1), group(c, 2))),
    );
    out = mask_each(
        &out,
        &r.bearer,
        SensitiveKind::BearerToken,
        &mut found,
        |c| Some(format!("{}{MASK}", group(c, 1))),
    );
    out = mask_each(
        &out,
        &r.labeled_en,
        SensitiveKind::LabeledValue,
        &mut found,
        |c| Some(format!("{}{MASK}", group(c, 1))),
    );
    out = mask_each(
        &out,
        &r.labeled_ja,
        SensitiveKind::LabeledValue,
        &mut found,
        |c| Some(format!("{}{MASK}", group(c, 1))),
    );
    // 3. 形式の決まった鍵（その一致全体を伏せる）
    for (kind, re) in &r.key_shapes {
        out = mask_each(&out, re, *kind, &mut found, |_| Some(MASK.to_string()));
    }
    out = mask_each(&out, &r.jwt, SensitiveKind::BearerToken, &mut found, |_| {
        Some(MASK.to_string())
    });
    out = mask_each(&out, &r.email, SensitiveKind::Email, &mut found, |_| {
        Some(MASK.to_string())
    });
    // 4. 個人情報の番号（カードは Luhn で確かめ、電話・連続 12 桁は数字の列の途中を除く）
    let card_text = out.clone();
    out = mask_each(&out, &r.card, SensitiveKind::CardNumber, &mut found, |c| {
        let m = c.get(0)?;
        if !digit_bounded(&card_text, m.start(), m.end()) {
            return None;
        }
        let digits: Vec<u32> = m
            .as_str()
            .chars()
            .filter_map(|ch| ch.to_digit(10))
            .collect();
        ((13..=19).contains(&digits.len()) && luhn_ok(&digits)).then(|| MASK.to_string())
    });
    let phone_text = out.clone();
    for re in [&r.phone_hyphen, &r.phone_plain] {
        out = mask_each(&out, re, SensitiveKind::Phone, &mut found, |c| {
            let m = c.get(0)?;
            digit_bounded(&phone_text, m.start(), m.end()).then(|| MASK.to_string())
        });
    }
    out = mask_each(
        &out,
        &r.phone_intl,
        SensitiveKind::Phone,
        &mut found,
        |_| Some(MASK.to_string()),
    );
    let number_text = out.clone();
    out = mask_each(
        &out,
        &r.twelve_digits,
        SensitiveKind::LongNumber,
        &mut found,
        |c| {
            let m = c.get(0)?;
            digit_bounded(&number_text, m.start(), m.end()).then(|| MASK.to_string())
        },
    );
    // 5. 高エントロピーの長い文字列（最後。前の段階で伏せた部分は英数字を含まない）
    let entropy_text = out.clone();
    out = mask_each(
        &out,
        &r.entropy,
        SensitiveKind::HighEntropy,
        &mut found,
        |c| {
            let m = c.get(0)?;
            high_entropy_token(&entropy_text, m.start(), m.end(), m.as_str())
                .then(|| MASK.to_string())
        },
    );
    (out, found)
}

/// ファイル名（パスの最後の要素）が機微なものか。大文字小文字と全角英数を区別しない。
///
/// 該当するファイルは、差分の本文も変更量も渡さない（ファイル名と変更の種類だけ）。
pub fn sensitive_file(path: &str) -> bool {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = normalize_name(base);
    const EXACT: [&str; 4] = [".npmrc", ".netrc", ".pgpass", "terraform.tfstate"];
    const PREFIX: [&str; 6] = [
        ".env",
        "id_rsa",
        "id_ed25519",
        "id_ecdsa",
        "id_dsa",
        "terraform.tfstate",
    ];
    const SUFFIX: [&str; 12] = [
        ".pem",
        ".key",
        ".pfx",
        ".p12",
        ".keystore",
        ".jks",
        ".kdbx",
        ".tfvars",
        ".sqlite",
        ".sqlite3",
        ".db",
        ".ppk",
    ];
    const CONTAINS: [&str; 14] = [
        "credential",
        "secret",
        "password",
        "passwd",
        "token",
        "apikey",
        "api_key",
        "api-key",
        "パスワード",
        "暗証",
        "秘密",
        "認証",
        "マイナンバー",
        "個人情報",
    ];
    EXACT.contains(&name.as_str())
        || PREFIX.iter().any(|p| name.starts_with(p))
        || SUFFIX.iter().any(|s| name.ends_with(s))
        || CONTAINS.iter().any(|c| name.contains(c))
}

/// 全角の英数と記号を半角にし、小文字にする（ファイル名の判定用）
fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// 差分の本文などの機微な文字列を伏せ字にする。戻り値は（伏せ字後の文字列, 伏せた数）。
///
/// 伏せるのは値の部分だけ（ラベルや前後の文は残す）。秘密鍵のブロックだけは、BEGIN から END までを 1 つの伏せ字にする。
pub fn mask_secrets(text: &str) -> (String, usize) {
    let (out, found) = scan(text);
    (out, found.len())
}

/// 1 行（出力の行・入力の行）に機微情報が含まれるときの種類。含まなければ `None`。
///
/// 伏せ字そのもの（`MASK`）が含まれる場合も該当とする（伏せ字を出力に出させないため）。
pub fn looks_sensitive(text: &str) -> Option<SensitiveKind> {
    if text.contains(MASK) {
        return Some(SensitiveKind::Masked);
    }
    scan(text).1.first().copied()
}

/// 保存欄のメモに含まれる機微情報の種類（行ごとに調べ、重複を除く。内容は返さない）。
pub fn memo_sensitive_kinds(text: &str) -> Vec<SensitiveKind> {
    let mut kinds = Vec::new();
    for line in text.lines() {
        if let Some(kind) = looks_sensitive(line) {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
    }
    kinds
}

/// 出力の行が、差分の本文の連続する `min_chars` 文字以上をそのまま含むか（長い写しの検出）。
///
/// 空白と句読点・記号を除き、大文字小文字を区別せずに比べる。日本語は文字単位。
pub fn copies_diff(line: &str, diff_text: &str, min_chars: usize) -> bool {
    if min_chars == 0 {
        return false;
    }
    let line_chars: Vec<char> = compact(line).chars().collect();
    if line_chars.len() < min_chars {
        return false;
    }
    let diff = compact(diff_text);
    line_chars
        .windows(min_chars)
        .any(|window| diff.contains(&window.iter().collect::<String>()))
}

/// 比較用に、英数字と日本語の文字だけを残し、小文字にする
fn compact(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 検出の種類を 1 つだけ確かめる
    fn kind_of(text: &str) -> Option<SensitiveKind> {
        looks_sensitive(text)
    }

    #[test]
    fn every_pattern_compiles() {
        // rules() の中の Regex::new は expect を使うため、初回に全件を作って確かめる
        let r = rules();
        assert!(r.key_shapes.len() == 5);
    }

    #[test]
    fn private_key_block_is_masked_as_one_mask() {
        let text = "+ -----BEGIN RSA PRIVATE KEY-----\n+ MIIEpAIBAAKCAQEAsynthetic\n+ -----END RSA PRIVATE KEY-----\n+ 次の行";
        let (out, n) = mask_secrets(text);
        assert_eq!(n, 1);
        assert!(!out.contains("MIIE"));
        assert!(out.contains("次の行"));
        assert!(out.contains(MASK));
        assert_eq!(
            kind_of("-----BEGIN OPENSSH PRIVATE KEY-----"),
            Some(SensitiveKind::PrivateKey)
        );
    }

    #[test]
    fn access_and_service_tokens_are_detected() {
        assert_eq!(
            kind_of("AKIAIOSFODNN7EXAMPLE"),
            Some(SensitiveKind::AccessKey)
        );
        assert_eq!(
            kind_of("ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
            Some(SensitiveKind::Token)
        );
        assert_eq!(
            kind_of("github_pat_11ABCDEFG0123456789_abcdef"),
            Some(SensitiveKind::Token)
        );
        assert_eq!(
            kind_of("xoxb-1234567890-abcdef"),
            Some(SensitiveKind::Token)
        );
        assert_eq!(
            kind_of("sk-abcdefghijklmnop1234"),
            Some(SensitiveKind::Token)
        );
        assert_eq!(
            kind_of("sk_live_abcdefghijklmnop1234"),
            Some(SensitiveKind::Token)
        );
        assert_eq!(
            kind_of("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.abcdefghij"),
            Some(SensitiveKind::BearerToken)
        );
        assert_eq!(
            kind_of("Authorization: Bearer abcdefghijklmnopqrstuv"),
            Some(SensitiveKind::BearerToken)
        );
    }

    #[test]
    fn short_or_unrelated_key_shapes_are_not_detected() {
        assert_eq!(kind_of("sk-abc"), None);
        assert_eq!(kind_of("task-manager-v2"), None);
        assert_eq!(kind_of("ghp_short"), None);
    }

    #[test]
    fn url_credentials_mask_only_the_password() {
        let (out, n) = mask_secrets("接続先 https://alice:hunter2x@example.com/path");
        assert_eq!(n, 1);
        assert_eq!(out, format!("接続先 https://alice:{MASK}@example.com/path"));
        assert_eq!(
            kind_of("https://alice:hunter2x@example.com"),
            Some(SensitiveKind::UrlCredential)
        );
    }

    #[test]
    fn labeled_values_keep_the_label_and_mask_the_value() {
        let (out, _) = mask_secrets("password = hunter2!\napi_key: synthetic-value-123");
        assert_eq!(out, format!("password = {MASK}\napi_key: {MASK}"));
        // JSON の形式（キーが引用符で囲まれている）も伏せる
        let (out, _) = mask_secrets(r#""password": "hunter2""#);
        assert!(!out.contains("hunter2"));
        // 日本語のラベル
        let (out, _) = mask_secrets("パスワード：abc123xyz です");
        assert_eq!(out, format!("パスワード：{MASK} です"));
        let (out, _) = mask_secrets("暗証番号=4321");
        assert_eq!(out, format!("暗証番号={MASK}"));
        assert_eq!(kind_of("SECRET=abc"), Some(SensitiveKind::LabeledValue));
    }

    #[test]
    fn ordinary_japanese_text_is_not_masked() {
        for text in [
            "契約書の支払期限を翌月末払いから変えた。",
            "税率を8%から10%に変更",
            "第3章の実験回数を5回に修正",
            "保存メモを書く画面の文言を直した",
            "認証の方式について補足を追記",
        ] {
            assert_eq!(mask_secrets(text), (text.to_string(), 0), "{text}");
            assert_eq!(kind_of(text), None, "{text}");
        }
    }

    #[test]
    fn file_paths_versions_hashes_and_postal_like_numbers_are_not_masked() {
        for text in [
            "docs/notes/phase2-llm-evaluation.md を更新",
            "src-tauri/crates/core-llm/src/redact.rs",
            "version 1.13.1 に固定",
            "commit 3f2a9c1 を取り消し",
            "郵便番号 100-0001 の欄",
            "1234567 件",
            "2026-10-12 の版",
        ] {
            assert_eq!(mask_secrets(text).1, 0, "{text}");
        }
    }

    #[test]
    fn email_phone_and_intl_phone_are_masked() {
        let (out, n) = mask_secrets("連絡先 test@example.com、電話 090-0000-0000");
        assert_eq!(n, 2);
        assert!(!out.contains("example.com"));
        assert!(!out.contains("0000-0000"));
        assert_eq!(kind_of("+81 90 0000 0000"), Some(SensitiveKind::Phone));
        assert_eq!(kind_of("09000000000"), Some(SensitiveKind::Phone));
        // 数字の列の途中は電話とみなさない
        assert_ne!(kind_of("12309000000001"), Some(SensitiveKind::Phone));
    }

    #[test]
    fn card_numbers_need_luhn_and_separators_are_allowed() {
        assert_eq!(
            kind_of("4242 4242 4242 4242"),
            Some(SensitiveKind::CardNumber)
        );
        assert_eq!(
            kind_of("4242-4242-4242-4242"),
            Some(SensitiveKind::CardNumber)
        );
        assert_eq!(kind_of("4242424242424242"), Some(SensitiveKind::CardNumber));
        // Luhn を満たさない 16 桁は、カードとしては伏せない（長い数字として 12 桁の列も含まない）
        assert_eq!(kind_of("4242 4242 4242 4241"), None);
        let (out, _) = mask_secrets("カード 4242 4242 4242 4242 を使用");
        assert_eq!(out, format!("カード {MASK} を使用"));
    }

    #[test]
    fn twelve_digit_numbers_are_masked_only_when_standalone() {
        assert_eq!(kind_of("123456789012"), Some(SensitiveKind::LongNumber));
        assert_eq!(kind_of("1234567890123"), None);
        assert_eq!(kind_of("郵便 1000001"), None);
    }

    #[test]
    fn high_entropy_strings_are_masked_but_paths_are_not() {
        let secret = "Qm9zYzV0ZXN0VmFsdWUxMjM0NTY3ODlhYmNkZWZH";
        assert_eq!(kind_of(secret), Some(SensitiveKind::HighEntropy));
        // 英字の 1 種類だけ（大文字だけ）は対象外
        assert_eq!(kind_of("ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFG"), None);
        // パス・URL の一部は対象外
        assert_eq!(
            kind_of("https://example.com/Qm9zYzV0ZXN0VmFsdWUxMjM0NTY3ODlhYmNkZWZH"),
            None
        );
        assert_eq!(
            kind_of("src/lib/Qm9zYzV0ZXN0VmFsdWUxMjM0NTY3ODlhYmNkZWZHabcdef/x.ts"),
            None
        );
    }

    #[test]
    fn the_mask_itself_counts_as_sensitive_in_output() {
        assert_eq!(
            looks_sensitive(&format!("文言 {MASK}")),
            Some(SensitiveKind::Masked)
        );
    }

    #[test]
    fn sensitive_file_names_are_detected_in_any_case_and_width() {
        for path in [
            ".env",
            "app/.env.production",
            "keys/server.pem",
            "id_rsa",
            "id_ed25519.pub",
            "cert.P12",
            "Ｄａｔａ.ＫＥＹ",
            ".npmrc",
            "infra/terraform.tfstate",
            "db/main.sqlite",
            "config/Secrets.json",
            "design/パスワード一覧.xlsx",
            "経理/暗証メモ.txt",
            "総務/マイナンバー.docx",
        ] {
            assert!(sensitive_file(path), "{path}");
        }
        for path in [
            "docs/design.md",
            "src/main.rs",
            "資料/第3章.docx",
            "notes/env-setup-guide.md",
            "data/tokenizer-notes.md",
        ] {
            // 「env-setup」は .env の接頭辞ではない。tokenizer は token を含むため対象になる点は仕様どおり
            if path.contains("tokenizer") {
                assert!(sensitive_file(path), "{path}");
            } else {
                assert!(!sensitive_file(path), "{path}");
            }
        }
    }

    #[test]
    fn copies_diff_detects_long_runs_only() {
        let diff = "+ 支払は月末締め翌々月15日払いとする\n- 支払は月末締め翌月末払いとする";
        // 12 文字以上の連続した写し
        assert!(copies_diff("支払は月末締め翌々月15日払い", diff, 12));
        // 句読点と空白の違いは無視する
        assert!(copies_diff("支払は、月末締め 翌々月15日払い", diff, 12));
        // 短い一致は写しとみなさない
        assert!(!copies_diff("支払を変えた", diff, 12));
        // 要約（24 文字）の基準では、24 文字未満の一致は写しとみなさない
        let long = "+ 支払は月末締め翌々月15日払いとする。違約金は未払い額の一割とする。";
        assert!(!copies_diff("支払は月末締め翌々月15日払い", long, 24));
        assert!(copies_diff(
            "支払は月末締め翌々月15日払いとする違約金は未払い",
            long,
            24
        ));
        // 英字は大文字小文字を区別しない
        assert!(copies_diff("ABCDEFGHIJKLMN", "+ abcdefghijklmn", 12));
    }

    #[test]
    fn memo_sensitive_kinds_collects_distinct_kinds_per_line() {
        let kinds = memo_sensitive_kinds(
            "連絡 test@example.com\n電話 090-0000-0000\nメール test@example.org",
        );
        assert_eq!(kinds, vec![SensitiveKind::Email, SensitiveKind::Phone]);
        assert!(memo_sensitive_kinds("通常の保存メモです").is_empty());
    }
}
