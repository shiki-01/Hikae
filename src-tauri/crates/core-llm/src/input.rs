// 保存メモの入力の組み立てと、キャッシュのキー（設計書 10.4 の 1〜2、5）。
//
// - Tauri・git に依存しない純関数。呼び出し側（app）が集めた変更を `SourceChange` にして渡す
// - 変更量・差分の文字列の形も、ここで決める（UI の DiffView と同じ「Sheet1!B3: 120 → 150」の形）
// - キャッシュのキーは「組み立てたメッセージ＋モデル ID」の SHA-256。作業フォルダの状態に依存する
//   入力そのものを基準にするため、ツリーの OID は使わない（設計書 10.4 の追記を参照）

use crate::prompt::{build_messages, ChangeInput, ChangeKind, ChangedFile};
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

/// 変更一覧から、モデルへ渡す入力を作る。ファイル名は最後の要素だけを使う（フォルダは渡さない）。
/// 件数と本文の上限は `prompt` 側で切り詰める。
pub fn change_input(changes: &[SourceChange]) -> ChangeInput {
    ChangeInput {
        files: changes
            .iter()
            .map(|c| ChangedFile {
                kind: c.kind,
                name: file_name(&c.path).to_string(),
                old_name: c.old_path.as_deref().map(|p| file_name(p).to_string()),
                amount: c.amount.clone(),
                diff_text: c.diff_text.clone(),
            })
            .collect(),
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
/// 同じ入力・同じモデルなら同じキーになり、どちらかが変われば別のキーになる。
pub fn cache_key(input: &ChangeInput, model_ref: &str) -> String {
    let payload = serde_json::json!({
        "model": model_ref,
        "messages": build_messages(input),
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
        let input = change_input(&[
            change(ChangeKind::Modified, "資料/第3章.docx"),
            SourceChange {
                kind: ChangeKind::Renamed,
                path: "a/新.txt".to_string(),
                old_path: Some("b/旧.txt".to_string()),
                amount: Some(text_amount(0, 0)),
                diff_text: None,
            },
        ]);
        assert_eq!(input.files.len(), 2);
        assert_eq!(input.files[0].name, "第3章.docx");
        assert_eq!(input.files[0].old_name, None);
        assert_eq!(input.files[0].amount, None);
        assert_eq!(input.files[1].name, "新.txt");
        assert_eq!(input.files[1].old_name.as_deref(), Some("旧.txt"));
        assert_eq!(input.files[1].amount.as_deref(), Some("行 +0 / -0"));
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
    fn cache_key_is_stable_and_follows_the_input_and_the_model() {
        let base = change_input(&[change(ChangeKind::Added, "a.txt")]);
        let key = cache_key(&base, "m1");
        assert_eq!(key.len(), 64);
        assert!(key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(key, cache_key(&base.clone(), "m1"));
        // モデルが違えば別のキー
        assert_ne!(key, cache_key(&base, "m2"));
        // 入力が違えば別のキー（差分本文だけの違いも含む）
        let mut other = base.clone();
        other.files[0].diff_text = Some("+ 新しい行".to_string());
        assert_ne!(key, cache_key(&other, "m1"));
    }
}
