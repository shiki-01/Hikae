// 拡張の `id` の形式（設計書 11.2）。マニフェスト（host）と索引（11.10。host の feature に依存しない）の両方が使うため、
// host の外に置く。

/// `id` の形式（設計書 11.2）: `^[a-z0-9]+([.-][a-z0-9]+)*$`
pub(crate) fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    if !alnum(first) || !alnum(last) {
        return false;
    }
    // 区切りの直後は英数字でなければならない（連続した区切りを拒否する）
    let mut after_separator = false;
    for &c in bytes {
        if alnum(c) {
            after_separator = false;
        } else if c == b'.' || c == b'-' {
            if after_separator {
                return false;
            }
            after_separator = true;
        } else {
            return false;
        }
    }
    true
}
