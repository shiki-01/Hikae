use crate::error::AuthError;
use crate::token::AccessToken;

/// OS キーチェーンを使用した GitHub トークン保管。
/// Service: "com.shiki01.hikae"、User: "github"
pub struct TokenStore;

impl TokenStore {
    const SERVICE: &'static str = "com.shiki01.hikae";
    const USER: &'static str = "github";

    /// トークンを OS キーチェーンに保存する。
    pub fn save(token: &AccessToken) -> Result<(), AuthError> {
        let entry =
            keyring::Entry::new(Self::SERVICE, Self::USER).map_err(|_| AuthError::KeyringError)?;

        entry
            .set_password(token.expose_secret())
            .map_err(|_| AuthError::FailedToSaveToken)
    }

    /// OS キーチェーンからトークンを読み込む。トークンが存在しない場合は None を返す。
    pub fn load() -> Result<Option<AccessToken>, AuthError> {
        let entry =
            keyring::Entry::new(Self::SERVICE, Self::USER).map_err(|_| AuthError::KeyringError)?;

        match entry.get_password() {
            Ok(password) => Ok(Some(AccessToken::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(AuthError::FailedToLoadToken),
        }
    }

    /// OS キーチェーンからトークンを削除する。
    pub fn delete() -> Result<(), AuthError> {
        let entry =
            keyring::Entry::new(Self::SERVICE, Self::USER).map_err(|_| AuthError::KeyringError)?;

        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(AuthError::FailedToDeleteToken),
        }
    }

    /// トークンが保存されているかを確認する（値は返さない）。
    pub fn exists() -> Result<bool, AuthError> {
        Self::load().map(|opt| opt.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // OS キーチェーンにアクセスするため、手動実行でのみ実行。CI では無視される。
    fn test_token_store_save_load() {
        // テスト用トークン（実在しない）
        let test_token = AccessToken::new("test_token_value_12345".to_string());

        // 既存トークンがあれば削除
        let _ = TokenStore::delete();

        // 保存
        TokenStore::save(&test_token).expect("save failed");

        // 読み込み
        let loaded = TokenStore::load().expect("load failed").expect("no token");
        assert_eq!(
            loaded.expose_secret(),
            test_token.expose_secret(),
            "loaded token should match saved token"
        );

        // 削除
        TokenStore::delete().expect("delete failed");

        // 確認（削除後は None）
        let deleted = TokenStore::load().expect("load after delete failed");
        assert!(deleted.is_none(), "token should be deleted");
    }

    #[test]
    #[ignore]
    fn test_token_store_exists() {
        // テスト用トークン
        let test_token = AccessToken::new("test_token_12345".to_string());

        // 削除
        let _ = TokenStore::delete();

        // 存在しない
        let exists = TokenStore::exists().expect("exists check failed");
        assert!(!exists, "token should not exist initially");

        // 保存
        TokenStore::save(&test_token).expect("save failed");

        // 存在する
        let exists = TokenStore::exists().expect("exists check failed");
        assert!(exists, "token should exist after save");

        // 削除
        TokenStore::delete().expect("delete failed");
    }
}
