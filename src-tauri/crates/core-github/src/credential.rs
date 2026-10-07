use crate::token::AccessToken;

/// Git credential protocol の `get` 応答を組み立てる純粋関数。
/// 出力にはトークンが含まれるため、ログには出さない。
/// フォーマット:
/// ```text
/// username=x-access-token
/// password=<token>
/// ```
pub fn credential_helper_output(_host: &str, token: &AccessToken) -> String {
    format!(
        "username=x-access-token\npassword={}\n",
        token.expose_secret()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_helper_output() {
        let token = AccessToken::new("test_token_value_123".to_string());
        let output = credential_helper_output("github.com", &token);

        // 出力に username と password が含まれること
        assert!(output.contains("username=x-access-token"));
        assert!(output.contains("password=test_token_value_123"));

        // 改行で終わること
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn test_credential_helper_output_token_not_in_format() {
        // トークン自体は Format トレイトで隠蔽されるが、
        // この関数の戻り値だけが実際の値を含む
        let token = AccessToken::new("secret_token".to_string());

        // Display/Debug には含まれない
        assert!(!format!("{}", token).contains("secret_token"));
        assert!(!format!("{:?}", token).contains("secret_token"));

        // しかし credential_helper_output には含まれる（これが正常）
        let output = credential_helper_output("github.com", &token);
        assert!(output.contains("secret_token"));
    }
}
