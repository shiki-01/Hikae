// トークンの保管（設計書 14.6）。iOS・デスクトップは OS のキーチェーン（keyring）、
// Android は Android Keystore で包んだ保管（tauri-plugin-hikae-secure-store）を使う。
// トークンはファイル・ログ・エラーに出さない（不変条件 9）。呼び出し側は使い終わったらすぐ捨てる。

use core_github::{AccessToken, AuthError};

#[cfg(not(target_os = "android"))]
use core_github::TokenStore;
#[cfg(target_os = "android")]
use tauri_plugin_hikae_secure_store::SecureStoreExt;

/// 保管先に付ける名前（Android 用。keyring のサービス名・ユーザー名と同じ役割）
#[cfg(target_os = "android")]
const KEY: &str = "github-token";

#[derive(Clone)]
pub struct Tokens {
    #[cfg(target_os = "android")]
    app: tauri::AppHandle,
}

impl Tokens {
    pub fn new(app: tauri::AppHandle) -> Self {
        #[cfg(not(target_os = "android"))]
        let _ = app;
        Self {
            #[cfg(target_os = "android")]
            app,
        }
    }

    #[cfg(not(target_os = "android"))]
    fn load_sync(&self) -> Result<Option<AccessToken>, AuthError> {
        TokenStore::load()
    }

    #[cfg(not(target_os = "android"))]
    fn save_sync(&self, token: &AccessToken) -> Result<(), AuthError> {
        TokenStore::save(token)
    }

    #[cfg(not(target_os = "android"))]
    fn delete_sync(&self) -> Result<(), AuthError> {
        TokenStore::delete()
    }

    #[cfg(target_os = "android")]
    fn load_sync(&self) -> Result<Option<AccessToken>, AuthError> {
        match self.app.secure_store().get(KEY) {
            Ok(value) => Ok(value.filter(|v| !v.is_empty()).map(AccessToken::new)),
            Err(_) => Err(AuthError::FailedToLoadToken),
        }
    }

    #[cfg(target_os = "android")]
    fn save_sync(&self, token: &AccessToken) -> Result<(), AuthError> {
        self.app
            .secure_store()
            .set(KEY, token.expose_secret())
            .map_err(|_| AuthError::FailedToSaveToken)
    }

    #[cfg(target_os = "android")]
    fn delete_sync(&self) -> Result<(), AuthError> {
        self.app
            .secure_store()
            .delete(KEY)
            .map_err(|_| AuthError::FailedToDeleteToken)
    }

    /// 保管先の読み書きは同期の処理なので、ブロッキング用のスレッドで行う
    async fn run<T, F>(&self, f: F) -> Result<T, AuthError>
    where
        T: Send + 'static,
        F: FnOnce(Tokens) -> Result<T, AuthError> + Send + 'static,
    {
        let this = self.clone();
        tauri::async_runtime::spawn_blocking(move || f(this))
            .await
            .map_err(|_| AuthError::KeyringError)?
    }

    pub async fn load(&self) -> Result<Option<AccessToken>, AuthError> {
        self.run(|t| t.load_sync()).await
    }

    pub async fn save(&self, token: AccessToken) -> Result<(), AuthError> {
        self.run(move |t| t.save_sync(&token)).await
    }

    pub async fn delete(&self) -> Result<(), AuthError> {
        self.run(|t| t.delete_sync()).await
    }

    /// 保管先の読み書きの自己テスト（CI のシミュレータ確認用。環境変数 `HIKAE_SELFTEST` があるときだけ呼ばれる）。
    /// すでにトークンがあるときは、利用者のトークンに触れないよう何もしない
    pub async fn selftest(&self) -> Result<String, String> {
        match self.load().await {
            Ok(Some(_)) => return Ok("skipped (token exists)".to_string()),
            Ok(None) => {}
            Err(e) => return Err(format!("load failed: {e:?}")),
        }
        let probe = "selftest-token-0123456789";
        self.save(AccessToken::new(probe.to_string()))
            .await
            .map_err(|e| format!("save failed: {e:?}"))?;
        let loaded = self
            .load()
            .await
            .map_err(|e| format!("load after save failed: {e:?}"))?;
        let same = loaded.as_ref().map(|t| t.expose_secret() == probe) == Some(true);
        self.delete()
            .await
            .map_err(|e| format!("delete failed: {e:?}"))?;
        let gone = matches!(self.load().await, Ok(None));
        if same && gone {
            Ok("ok".to_string())
        } else {
            Err(format!("mismatch (same={same}, gone={gone})"))
        }
    }
}
