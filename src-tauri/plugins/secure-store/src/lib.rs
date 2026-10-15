// Android Keystore で包んだ小さな秘密情報の保管（設計書 14.6）。
// 値は AES-256-GCM で暗号化して SharedPreferences に置き、鍵は Android Keystore から出さない。
// コマンドは画面（JS）に公開しない。Rust（hikae-mobile）の `SecureStore` からだけ使う。
// Android 以外では `Error::Unsupported` を返す（iOS は keyring、デスクトップは keyring を使う）。

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.shiki01.hikae.securestore";

/// 保管の失敗。値や鍵の内容は含めない
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("この環境では安全な保管先を使えません")]
    Unsupported,
    #[error("安全な保管先の操作に失敗しました")]
    Plugin,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct SecureStore<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _marker: std::marker::PhantomData<fn() -> R>,
}

#[cfg(target_os = "android")]
mod android_types {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize)]
    pub struct KeyArgs<'a> {
        pub key: &'a str,
    }

    #[derive(Serialize)]
    pub struct SetArgs<'a> {
        pub key: &'a str,
        pub value: &'a str,
    }

    #[derive(Deserialize)]
    pub struct GetResponse {
        #[serde(default)]
        pub value: Option<String>,
    }
}

#[cfg(target_os = "android")]
impl<R: Runtime> SecureStore<R> {
    /// 値を読む。無い、または復号できない（バックアップの復元で鍵が無いなど）ときは `None`
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let res: android_types::GetResponse = self
            .handle
            .run_mobile_plugin("get", android_types::KeyArgs { key })
            .map_err(|_| Error::Plugin)?;
        Ok(res.value)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let _: serde_json::Value = self
            .handle
            .run_mobile_plugin("set", android_types::SetArgs { key, value })
            .map_err(|_| Error::Plugin)?;
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let _: serde_json::Value = self
            .handle
            .run_mobile_plugin("delete", android_types::KeyArgs { key })
            .map_err(|_| Error::Plugin)?;
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
impl<R: Runtime> SecureStore<R> {
    pub fn get(&self, _key: &str) -> Result<Option<String>> {
        Err(Error::Unsupported)
    }

    pub fn set(&self, _key: &str, _value: &str) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn delete(&self, _key: &str) -> Result<()> {
        Err(Error::Unsupported)
    }
}

/// `app.secure_store()` で取り出す
pub trait SecureStoreExt<R: Runtime> {
    fn secure_store(&self) -> &SecureStore<R>;
}

impl<R: Runtime, T: Manager<R>> SecureStoreExt<R> for T {
    fn secure_store(&self) -> &SecureStore<R> {
        self.state::<SecureStore<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("hikae-secure-store")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let store = SecureStore::<R> {
                handle: api.register_android_plugin(PLUGIN_IDENTIFIER, "SecureStorePlugin")?,
            };
            #[cfg(not(target_os = "android"))]
            let store = {
                let _ = &api;
                SecureStore::<R> {
                    _marker: std::marker::PhantomData,
                }
            };
            app.manage(store);
            Ok(())
        })
        .build()
}
