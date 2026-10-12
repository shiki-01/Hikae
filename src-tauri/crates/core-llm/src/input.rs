// 保存メモの入力の組み立てと、キャッシュのキー（設計書 10.4 の 1〜2、5。10.6 の入力側の対策）。
//
// - Tauri・git に依存しない純関数。呼び出し側（app）が集めた変更を `SourceChange` にして渡す
// - 変更量・差分の文字列の形も、ここで決める（UI の DiffView と同じ「Sheet1!B3: 120 → 150」の形）
// - 機微なファイル（`sensitive_file`）は本文も変更量も渡さない。差分の本文と名前は `mask_secrets` で伏せ字にする
// - キャッシュのキーは「組み立てたメッセージ＋モデル ID」の SHA-256。伏せ字の後の入力から作るため、
//   元の機微な文字列はキーにもメッセージにも残らない

use crate::prompt::{build_messages, ChangeInput, ChangeKind, ChangedFile};
use crate::redact::{mask_secrets, sensitive_file};
use crate::suggestion::SuggestNotice;
use sha2::{Digest, Sha256};

/// 呼び出し側が集めた変更 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChange {
    pub kind: ChangeKind,
    /// プロジェクト相対パス（`/` 区切り）
    pub path: String,
    /// 名前変更の旧パス
    pub old_path: Option<String>,
    /// 変更量の要約（`text_amount` などで作る）。None のときは「-」として渡す
    pub amount: Option<String>,
    /// 抽出した差分テキスト（行ごとに `+ ` / `- ` の接頭辞を付けたもの）。None は本文を渡さない
    pub diff_text: Option<String>,
}

/// モデルへ渡す入力と、利用者に出す注記
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub input: ChangeInput,
    pub notices: Vec<SuggestNotice>,
}

/// 変更一覧から、伏せ字を施したモデルへの入力と注記を作る。ファイル名は最後の要素だけを使う（フォルダは渡さない）。
///
/// - 機微なファイル（`sensitive_file` が真）は、本文も変更量も渡さない（名前と種類だけ）。注記 `sensitive-file-skipped`
/// - 名前と本文は `mask_secrets` で伏せ字にする。1 件でも伏せたら注記 `sensitive-masked`
/// - 件数と本文の上限は `prompt` 側で切り詰める
pub fn prepare_input(changes: &[SourceChange]) -> Prepared {
    let mut masked = false;
    let mut skipped = false;
    let mut mask = |text: &str| -> String {
        let (out, n) = mask_secrets(text);
        if n > 0 {
            masked = true;
        }
        out
    };
    let mut files = Vec::with_capacity(changes.len());
    for change in changes {
        let sensitive =
            sensitive_file(&change.path) || change.old_path.as_deref().is_some_and(sensitive_file);
        if sensitive {
            skipped = true;
        }
        let name = mask(file_name(&change.path));
        let old_name = change.old_path.as_deref().map(|p| mask(file_name(p)));
        files.push(ChangedFile {
            kind: change.kind,
            name,
            old_name,
            amount: if sensitive {
                None
            } else {
                change.amount.clone()
            },
            diff_text: if sensitive {
                None
            } else {
                change.diff_text.as_deref().map(&mut mask)
            },
            sensitive,
        });
    }
    let mut notices = Vec::new();
    if masked {
        notices.push(SuggestNotice::SensitiveMasked);
    }
    if skipped {
        notices.push(SuggestNotice::SensitiveFileSkipped);
    }
    Prepared {
        input: ChangeInput { files },
        notices,
    }
}

/// テキストの変更量（「行 +2 / -1」）
pub fn text_amount(added: usize, removed: usize) -> String {
    format!("行 +{added} / -{removed}")
}

/// docx の変更量（「段落 +2 / -1」）
pub fn paragraph_amount(added: usize, removed: usize) -> String {
    format!("段落 +{added} / -{removed}")
}

/// xlsx の変更量（「セル 3 件」）
pub fn cell_amount(count: usize) -> String {
    format!("セル {count} 件")
}

/// 追加・削除の 1 行（`+ 本文` / `- 本文`）
pub fn signed_line(added: bool, text: &str) -> String {
    format!("{} {}", if added { "+" } else { "-" }, text)
}

/// セルの変更 1 件（「Sheet1!B4: 120000 → 135000」）。無い側は空欄にする
pub fn cell_line(sheet: &str, cell: &str, before: Option<&str>, after: Option<&str>) -> String {
    format!(
        "{sheet}!{cell}: {} → {}",
        before.unwrap_or(""),
        after.unwrap_or("")
    )
}

/// 入力とモデル ID から、生成済みメモのキャッシュのキーを作る（小文字の 16 進 64 桁）。
/// 同じ入力・同じモデル・同じ形式（要約のみか本文もか）なら同じキーになり、どれかが変われば別のキーになる。
/// `body` は、プロンプトが変わることを表すため、メッセージに含まれる（キーの計算に使うのは伏せ字の後の入力だけ）。
pub fn cache_key(input: &ChangeInput, model_ref: &str, body: bool) -> String {
    let payload = serde_json::json!({
        "model": model_ref,
        "messages": build_messages(input, body),
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest.iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// パスの最後の要素（ファイル名）
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::MASK;

    fn change(kind: ChangeKind, path: &str) -> SourceChange {
        SourceChange {
            kind,
            path: path.to_string(),
            old_path: None,
            amount: None,
            diff_text: None,
        }
    }

    #[test]
    fn names_are_the_last_path_element_and_order_is_kept() {
        let prepared = prepare_input(&[
            change(ChangeKind::Modified, "資料/第3章.docx"),
            SourceChange {
                kind: ChangeKind::Renamed,
                path: "a/新.txt".to_string(),
                old_path: Some("b/旧.txt".to_string()),
                amount: Some(text_amount(0, 0)),
                diff_text: None,
            },
        ]);
        let input = prepared.input;
        assert_eq!(input.files.len(), 2);
        assert_eq!(input.files[0].name, "第3章.docx");
        assert_eq!(input.files[0].old_name, None);
        assert_eq!(input.files[0].amount, None);
        assert_eq!(input.files[1].name, "新.txt");
        assert_eq!(input.files[1].old_name.as_deref(), Some("旧.txt"));
        assert_eq!(input.files[1].amount.as_deref(), Some("行 +0 / -0"));
        assert!(prepared.notices.is_empty());
    }

    #[test]
    fn amounts_and_lines_have_fixed_shapes() {
        assert_eq!(text_amount(2, 1), "行 +2 / -1");
        assert_eq!(paragraph_amount(0, 3), "段落 +0 / -3");
        assert_eq!(cell_amount(4), "セル 4 件");
        assert_eq!(signed_line(true, "追加"), "+ 追加");
        assert_eq!(signed_line(false, "削除"), "- 削除");
        assert_eq!(
            cell_line("Sheet1", "B4", Some("120000"), Some("135000")),
            "Sheet1!B4: 120000 → 135000"
        );
        assert_eq!(
            cell_line("Sheet1", "C2", None, Some("x")),
            "Sheet1!C2:  → x"
        );
    }

    #[test]
    fn sensitive_files_send_no_body_and_no_amount() {
        let mut secret = change(ChangeKind::Added, "config/.env");
        secret.amount = Some(text_amount(2, 0));
        secret.diff_text = Some("+ DATABASE_URL=postgres://a:b@host/db".to_string());
        let prepared = prepare_input(&[secret]);
        let file = &prepared.input.files[0];
        assert!(file.sensitive);
        assert_eq!(file.name, ".env");
        assert_eq!(file.amount, None);
        assert_eq!(file.diff_text, None);
        assert_eq!(prepared.notices, vec![SuggestNotice::SensitiveFileSkipped]);
    }

    #[test]
    fn secrets_in_the_diff_are_masked_before_the_input_is_built() {
        let mut change_text = change(ChangeKind::Modified, "設定/接続.txt");
        change_text.amount = Some(text_amount(1, 0));
        change_text.diff_text = Some(
            "+ password = hunter2!\n+ 連絡先 test@example.com\n+ 接続の設定を変えた".to_string(),
        );
        let prepared = prepare_input(&[change_text]);
        let text = prepared.input.files[0]
            .diff_text
            .clone()
            .unwrap_or_default();
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("example.com"));
        assert!(text.contains("接続の設定を変えた"));
        assert!(text.contains(MASK));
        assert_eq!(prepared.notices, vec![SuggestNotice::SensitiveMasked]);
        assert!(!prepared.input.files[0].sensitive);
    }

    #[test]
    fn masked_input_gives_the_same_key_as_the_masked_text() {
        // キーは伏せ字の後の入力から作る。元の文字列は入力にも、キーの計算にも残らない
        let mut raw = change(ChangeKind::Modified, "a.txt");
        raw.diff_text = Some("+ password = hunter2!".to_string());
        let prepared = prepare_input(&[raw]);
        let key = cache_key(&prepared.input, "m1", true);
        let serialized = format!("{:?}", prepared.input);
        assert!(!serialized.contains("hunter2"));
        assert_eq!(key, cache_key(&prepared.input.clone(), "m1", true));
    }

    #[test]
    fn cache_key_is_stable_and_follows_the_input_the_model_and_the_form() {
        let base = prepare_input(&[change(ChangeKind::Added, "a.txt")]).input;
        let key = cache_key(&base, "m1", true);
        assert_eq!(key.len(), 64);
        assert!(key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(key, cache_key(&base.clone(), "m1", true));
        // モデルが違えば別のキー
        assert_ne!(key, cache_key(&base, "m2", true));
        // 要約のみと本文ありでは別のキー（プロンプトが違う）
        assert_ne!(key, cache_key(&base, "m1", false));
        // 入力が違えば別のキー（差分本文だけの違いも含む）
        let mut other = base.clone();
        other.files[0].diff_text = Some("+ 新しい行".to_string());
        assert_ne!(key, cache_key(&other, "m1", true));
    }
}
