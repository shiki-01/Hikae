// 保存時の署名（user.name / user.email）を決める純関数。

use crate::models::Identity;

/// 未ログイン時に使うローカル専用の署名名
pub const FALLBACK_NAME: &str = "Hikae";
/// 未ログイン時に使うローカル専用の署名メールアドレス
pub const FALLBACK_EMAIL: &str = "hikae@users.noreply.github.com";

/// ログイン済みのユーザー（GitHub の数値 ID とログイン名）から署名を決める。
/// ユーザーが分からない、またはログイン名が空・制御文字を含む場合は、ローカル専用の既定を返す。
pub fn resolve_identity(user: Option<(u64, &str)>) -> Identity {
    if let Some((id, login)) = user {
        let login = login.trim();
        if !login.is_empty() && !login.chars().any(char::is_control) {
            return Identity {
                name: login.to_string(),
                email: format!("{id}+{login}@users.noreply.github.com"),
            };
        }
    }
    Identity {
        name: FALLBACK_NAME.to_string(),
        email: FALLBACK_EMAIL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logged_in_user_uses_numeric_id_and_login() {
        let i = resolve_identity(Some((12345, "alice")));
        assert_eq!(i.name, "alice");
        assert_eq!(i.email, "12345+alice@users.noreply.github.com");
    }

    #[test]
    fn no_user_falls_back_to_local_default() {
        let i = resolve_identity(None);
        assert_eq!(i.name, FALLBACK_NAME);
        assert_eq!(i.email, FALLBACK_EMAIL);
    }

    #[test]
    fn invalid_login_falls_back() {
        for bad in ["", "   ", "a\nb"] {
            let i = resolve_identity(Some((1, bad)));
            assert_eq!(i.name, FALLBACK_NAME, "login={bad:?}");
            assert_eq!(i.email, FALLBACK_EMAIL);
        }
    }

    #[test]
    fn login_is_trimmed() {
        let i = resolve_identity(Some((7, " bob ")));
        assert_eq!(i.name, "bob");
        assert_eq!(i.email, "7+bob@users.noreply.github.com");
    }
}
