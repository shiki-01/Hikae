// URL に入れる値のエンコードと、ID の形の検査。依存を増やさないための自前の小関数（設計書 14.5）。

/// 英数字と `-_.~` 以外を `%XX` にする（クエリの値と、パスの 1 階層に使う）。
pub(crate) fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `/` 区切りのパスを、階層ごとにエンコードする。
pub(crate) fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// コミット・blob・tree の ID（16 進数の 40 桁か 64 桁）かどうか。
pub(crate) fn is_object_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_unreserved_and_encodes_the_rest() {
        assert_eq!(encode_component("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode_component("a b"), "a%20b");
        assert_eq!(encode_component("a?b#c"), "a%3Fb%23c");
        assert_eq!(encode_component("日本"), "%E6%97%A5%E6%9C%AC");
        assert_eq!(encode_component("a,b"), "a%2Cb");
    }

    #[test]
    fn path_keeps_separators() {
        assert_eq!(encode_path("docs/a b/c#d.md"), "docs/a%20b/c%23d.md");
    }

    #[test]
    fn object_ids_are_hex_of_sha1_or_sha256_length() {
        assert!(is_object_id(&"a".repeat(40)));
        assert!(is_object_id(&"0F".repeat(32)));
        assert!(!is_object_id(&"a".repeat(39)));
        assert!(!is_object_id(&"g".repeat(40)));
        assert!(!is_object_id(&format!("{}/x", "a".repeat(38))));
    }
}
