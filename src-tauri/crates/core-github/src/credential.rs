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

/// git が credential helper に渡す要求（`key=value` の行。空行で終わる）を読み、
/// 「https で github.com 宛て」のときだけ true を返す。
/// 他のホスト（偽の remote URL など）にトークンを渡さないための確認。
pub fn is_github_https_request(input: &str) -> bool {
    let mut protocol = None;
    let mut host = None;
    for line in input.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "protocol" => protocol = Some(value),
                "host" => host = Some(value),
                _ => {}
            }
        }
    }
    protocol == Some("https") && host.is_some_and(|h| h.eq_ignore_ascii_case("github.com"))
}

/// credential helper としての応答を決める。
/// `get` かつ github.com 宛てで、トークンがあるときだけ応答を返す。
/// `store` / `erase` は何もしない（トークンはアプリのログインでのみ保存・削除する）。
pub fn respond_to_git(operation: &str, input: &str, token: Option<&AccessToken>) -> Option<String> {
    if operation != "get" || !is_github_https_request(input) {
        return None;
    }
    token.map(|t| credential_helper_output("github.com", t))
}

/// git の `credential.helper` に渡す、アプリ自身を指すコマンド文字列を作る。
/// 形式: `!'<実行ファイル>' credential`（git は末尾に `get` などを付けて sh で実行する）。
/// Windows のパス区切りは sh で解釈されないため `/` に直す。
/// 引用符や制御文字を含むパスは安全に扱えないため None（認証なしになり、認証エラーとして失敗する）。
pub fn helper_command(exe_path: &str) -> Option<String> {
    if exe_path.is_empty() || exe_path.contains('\'') || exe_path.chars().any(char::is_control) {
        return None;
    }
    Some(format!("!'{}' credential", exe_path.replace('\\', "/")))
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

    #[test]
    fn helper_answers_only_get_for_github_https() {
        let token = AccessToken::new("tok_abc".to_string());
        let req = "protocol=https\nhost=github.com\n\n";
        let out = respond_to_git("get", req, Some(&token)).expect("response");
        assert!(out.contains("password=tok_abc"));

        // 他のホスト、http、別名のホストには渡さない
        for bad in [
            "protocol=https\nhost=evil.example\n\n",
            "protocol=https\nhost=github.com.evil.example\n\n",
            "protocol=http\nhost=github.com\n\n",
            "host=github.com\n\n",
            "",
        ] {
            assert_eq!(respond_to_git("get", bad, Some(&token)), None, "{bad:?}");
        }
        // store / erase は何もしない。トークンが無ければ応答しない
        assert_eq!(respond_to_git("store", req, Some(&token)), None);
        assert_eq!(respond_to_git("erase", req, Some(&token)), None);
        assert_eq!(respond_to_git("get", req, None), None);
        // 空行より後ろの行は読まない
        assert_eq!(
            respond_to_git("get", "protocol=https\n\nhost=github.com\n", Some(&token)),
            None
        );
        // CRLF でも読める
        assert!(respond_to_git(
            "get",
            "protocol=https\r\nhost=GitHub.com\r\n\r\n",
            Some(&token)
        )
        .is_some());
    }

    #[test]
    fn helper_command_quotes_and_normalizes_the_path() {
        assert_eq!(
            helper_command("C:\\Program Files\\Hikae\\hikae.exe").as_deref(),
            Some("!'C:/Program Files/Hikae/hikae.exe' credential")
        );
        assert_eq!(
            helper_command("/Applications/Hikae.app/Contents/MacOS/hikae").as_deref(),
            Some("!'/Applications/Hikae.app/Contents/MacOS/hikae' credential")
        );
        assert_eq!(helper_command("C:/it's/hikae.exe"), None);
        assert_eq!(helper_command("C:/a\nb/hikae.exe"), None);
        assert_eq!(helper_command(""), None);
    }
}
