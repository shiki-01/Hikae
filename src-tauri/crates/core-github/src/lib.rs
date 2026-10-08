// GitHub OAuth App Device Flow 認証と API 統合。トークン管理は keyring（OS キーチェーン）で行う。

pub mod api;
pub mod credential;
pub mod device_flow;
pub mod error;
mod http;
pub mod login;
pub mod store;
pub mod token;
pub mod user;

// 公開 API
pub use api::{
    clone_url_for, suggest_repository_name, validate_owner_login, validate_repository_name,
    CreateBlockReason, GithubApi, Owner, OwnerKind, RemoteRepo, RepoList,
};
pub use credential::{credential_helper_output, helper_command, respond_to_git};
pub use device_flow::{client_id_from_env, resolve_client_id, DeviceFlowClient};
pub use error::AuthError;
pub use login::{is_login_page_url, LoginCoordinator, LoginEnd, LoginPrompt, LOGIN_PAGE_URL};
pub use store::TokenStore;
pub use token::{AccessToken, DeviceCode, User};
pub use user::UserClient;
