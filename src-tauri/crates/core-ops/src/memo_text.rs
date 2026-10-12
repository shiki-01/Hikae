// 保存メモの正規化（設計書 4.1 の E）。保存の入口で 1 回だけ行い、commit メッセージにはこの結果を使う。
//
// - 純関数のみ。作業フォルダ・インデックス・refs には触れない
// - 改行は LF にそろえ、NUL と制御文字（改行・タブを除く）を除く。タブは空白にする
// - 行末の空白を削り、連続する空行を 1 つにし、全体の前後の空行と空白を削る
// - 上限（全体 4,000 文字・100 行）を超えるものは拒否する（黙って切り詰めない）

/// 保存メモの上限（文字数。正規化の後）
pub const MEMO_MAX_CHARS: usize = 4000;
/// 保存メモの上限（行数。正規化の後）
pub const MEMO_MAX_LINES: usize = 100;

/// 保存メモを正規化する。上限を超えるとき、または正規化の後に空になるときは `None`。
///
/// 空のメモは `None` にしない（空の扱いは従来どおり、呼び出し側と commit の処理に任せる）。
/// よって、戻り値が `None` になるのは上限の超過だけ。
pub fn normalize_memo(raw: &str) -> Option<String> {
    let unified = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for line in unified.split('\n') {
        let cleaned: String = line
            .chars()
            .filter_map(|c| match c {
                '\0' => None,
                '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect();
        let cleaned = cleaned.trim_end().to_string();
        // 連続する空行は 1 つにする
        if cleaned.is_empty() && lines.last().is_some_and(|last| last.is_empty()) {
            continue;
        }
        lines.push(cleaned);
    }
    // 前後の空行を取り除く
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    if lines.len() > MEMO_MAX_LINES {
        return None;
    }
    let text = lines.join("\n");
    if text.chars().count() > MEMO_MAX_CHARS {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_and_control_characters_are_normalized() {
        assert_eq!(
            normalize_memo("一行目\r\n二行目\r三行目\0\u{7}").as_deref(),
            Some("一行目\n二行目\n三行目")
        );
        assert_eq!(normalize_memo("A\tB").as_deref(), Some("A B"));
    }

    #[test]
    fn surrounding_blanks_are_trimmed_and_blank_runs_collapse_to_one() {
        assert_eq!(
            normalize_memo("\n\n  要約  \n\n\n\n本文 \n\n").as_deref(),
            Some("  要約\n\n本文")
        );
        assert_eq!(normalize_memo("   \n\n ").as_deref(), Some(""));
        assert_eq!(normalize_memo("").as_deref(), Some(""));
    }

    #[test]
    fn limits_are_enforced_without_truncating() {
        let lines: String = (0..MEMO_MAX_LINES).map(|i| format!("{i}\n")).collect();
        // 100 行ちょうどは通す（末尾の改行は空行とみなして落とす）
        assert!(normalize_memo(&lines).is_some());
        let too_many: String = (0..=MEMO_MAX_LINES).map(|i| format!("{i}\n")).collect();
        assert_eq!(normalize_memo(&too_many), None);
        assert!(normalize_memo(&"あ".repeat(MEMO_MAX_CHARS)).is_some());
        assert_eq!(normalize_memo(&"あ".repeat(MEMO_MAX_CHARS + 1)), None);
    }

    #[test]
    fn trailer_like_lines_stay_text_and_only_blank_lines_change() {
        // トレーラーに似た行も文字として残す（偽造の防止は pc_name の分離と改行の除去で行う）
        assert_eq!(
            normalize_memo("要約\n\n\nHikae-PC: x").as_deref(),
            Some("要約\n\nHikae-PC: x")
        );
    }
}
