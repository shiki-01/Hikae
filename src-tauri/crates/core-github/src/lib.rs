// GitHub OAuth App Device Flow 認証と API 統合。トークン管理は keyring（OS キーチェーン）で行う。

pub mod credential;
pub mod device_flow;
pub mod error;
pub mod store;
pub mod token;
pub mod user;

// 公開 API
pub use credential::credential_helper_output;
pub use device_flow::{client_id_from_env, DeviceFlowClient};
pub use error::AuthError;
pub use store::TokenStore;
pub use token::{AccessToken, DeviceCode, User};
pub use user::UserClient;
