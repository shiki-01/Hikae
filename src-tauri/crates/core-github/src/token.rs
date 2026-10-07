use std::fmt;

/// GitHub アクセストークン。値は隠蔽され、expose_secret() でのみ取得可能。
/// Debug と Display は値を出さない。serde::Serialize は実装しない。
#[derive(Clone)]
pub struct AccessToken(String);

impl AccessToken {
    pub fn new(token: String) -> Self {
        Self(token)
    }

    /// トークンの平文を取得する。この関数の戻り値のみがトークンを含む可能性がある。
    /// ログ出力や error メッセージには使わない。
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(***)")
    }
}

impl fmt::Display for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(***)")
    }
}

/// Device Flow の初期ステップで返される device code と verification info。
#[derive(Clone)]
pub struct DeviceCode {
    /// GitHub が割り当てた device code。トークン取得ポーリング時に使う。
    device_code: String,

    /// ユーザーが GitHub で入力する code。表示用。
    pub user_code: String,

    /// ユーザーが訪問する URL。表示用。
    pub verification_uri: String,

    /// トークン有効期限（秒）。この時間内にポーリングを完了する必要がある。
    pub expires_in: u64,

    /// ポーリング間隔（秒）。
    pub interval: u64,
}

impl DeviceCode {
    pub fn new(
        device_code: String,
        user_code: String,
        verification_uri: String,
        expires_in: u64,
        interval: u64,
    ) -> Self {
        Self {
            device_code,
            user_code,
            verification_uri,
            expires_in,
            interval,
        }
    }

    pub fn device_code(&self) -> &str {
        &self.device_code
    }
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceCode")
            .field("device_code", &"***")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

impl fmt::Display for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "user_code={}, verification_uri={}",
            self.user_code, self.verification_uri
        )
    }
}

/// GitHub ユーザー情報。noreply メールアドレスの生成に使う。
#[derive(Clone, Debug)]
pub struct User {
    /// GitHub ユーザー ID（数値）。noreply アドレス組み立て用。
    pub id: u64,

    /// GitHub ログイン名。noreply アドレス組み立て用。
    pub login: String,
}

impl User {
    pub fn new(id: u64, login: String) -> Self {
        Self { id, login }
    }

    /// noreply メールアドレスを生成する。
    /// フォーマット: <id>+<login>@users.noreply.github.com
    pub fn noreply_email(&self) -> String {
        format!("{}+{}@users.noreply.github.com", self.id, self.login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_hides_value() {
        let token = AccessToken::new("secret123".to_string());
        let debug_str = format!("{:?}", token);
        assert_eq!(debug_str, "AccessToken(***)");
        assert!(!debug_str.contains("secret123"));
    }

    #[test]
    fn token_display_hides_value() {
        let token = AccessToken::new("secret123".to_string());
        let display_str = format!("{}", token);
        assert_eq!(display_str, "AccessToken(***)");
        assert!(!display_str.contains("secret123"));
    }

    #[test]
    fn token_expose_secret_returns_value() {
        let token = AccessToken::new("secret123".to_string());
        assert_eq!(token.expose_secret(), "secret123");
    }

    #[test]
    fn user_noreply_email() {
        let user = User::new(12345, "alice".to_string());
        assert_eq!(user.noreply_email(), "12345+alice@users.noreply.github.com");
    }

    #[test]
    fn device_code_hides_device_code_in_debug() {
        let dc = DeviceCode::new(
            "hidden_device_code".to_string(),
            "ABC-1234".to_string(),
            "https://github.com/login/device".to_string(),
            900,
            5,
        );
        let debug_str = format!("{:?}", dc);
        assert!(debug_str.contains("ABC-1234"));
        assert!(!debug_str.contains("hidden_device_code"));
    }
}
