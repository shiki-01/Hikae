// GitHub REST API（オーナー一覧・リポジトリ一覧）。
//
// 認証は Bearer トークンをヘッダで渡す（URL やクエリには載せない）。
// エラーは `AuthError` の分類（失効・権限不足・通信不可・GitHub 側の障害）にだけ変換し、
// 応答本文や URL はエラーに含めない。

use crate::error::AuthError;
use crate::http;
use crate::token::AccessToken;
use crate::user::UserClient;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

/// 1 ページあたりの取得件数（GitHub の上限）
const DEFAULT_PAGE_SIZE: usize = 100;
/// 取得するページ数の上限（これを超える分は「一部のみ」として扱う）
const DEFAULT_MAX_PAGES: u32 = 10;

/// 保存先（オーナー）の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    /// 自分のアカウント
    Personal,
    /// 所属している Organization
    Org,
}

/// 保存先に新しいリポジトリを作れない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateBlockReason {
    /// Organization への参加がまだ承認されていない
    PendingInvitation,
    /// Organization の設定で、メンバーによるリポジトリ作成が許可されていない
    MembersCannotCreate,
    /// Organization の情報を確認できない（メンバーでない、または OAuth アプリの利用が許可されていない）
    NoAccess,
}

/// 保存先（個人または Organization）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// GitHub のログイン名（個人名または Organization 名）
    pub login: String,
    pub kind: OwnerKind,
    pub avatar_url: Option<String>,
    /// 新しいリポジトリを作れるか
    pub can_create: bool,
    /// 作れない場合の理由
    pub block_reason: Option<CreateBlockReason>,
}

/// 同名の既存リポジトリに接続してよいかの判定結果（設計書 4.6）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryAdoption {
    /// 保存（commit）が 1 つも無い空のリポジトリで、自分が書き込める。接続を提案してよい
    Adoptable(RemoteRepo),
    /// 見つからない（存在しない、または見えない）
    NotFound,
    /// 書き込めない（読み取り専用、アーカイブ済みなど）
    NoWriteAccess,
    /// すでに保存がある。他のデータを上書きしないため、決して自動では接続しない
    NotEmpty,
}

/// 取得できるリポジトリ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    /// `owner/name`
    pub full_name: String,
    pub name: String,
    /// 所有者のログイン名
    pub owner_login: String,
    /// 非公開か
    pub private: bool,
    /// 最終更新日時（RFC3339）
    pub updated_at: Option<String>,
}

/// リポジトリ一覧の取得結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoList {
    pub repos: Vec<RemoteRepo>,
    /// ページ数の上限に達し、これ以上の件数がある可能性がある
    pub truncated: bool,
}

#[derive(Deserialize)]
struct OrgSummary {
    login: String,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Membership {
    state: String,
    role: String,
}

#[derive(Deserialize)]
struct OrgDetail {
    #[serde(default)]
    members_can_create_repositories: Option<bool>,
}

#[derive(Deserialize)]
struct RepoOwner {
    login: String,
}

#[derive(Deserialize)]
struct RepoResponse {
    name: String,
    full_name: String,
    private: bool,
    owner: RepoOwner,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Deserialize, Default)]
struct RepoPermissions {
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    maintain: bool,
    #[serde(default)]
    push: bool,
}

/// `GET /repos/{owner}/{name}` の応答のうち、接続の判定に使う項目
#[derive(Deserialize)]
struct RepoDetail {
    name: String,
    full_name: String,
    private: bool,
    owner: RepoOwner,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    disabled: bool,
    /// 大きさ（KB）。保存が無い空のリポジトリは 0
    #[serde(default)]
    size: u64,
    #[serde(default)]
    permissions: Option<RepoPermissions>,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

/// 保存の有無の確認結果
enum EmptyProbe {
    Empty,
    NotEmpty,
    NotFound,
    NoAccess,
}

/// GitHub REST API クライアント。
pub struct GithubApi {
    client: Client,
    base: String,
    page_size: usize,
    max_pages: u32,
}

impl GithubApi {
    /// 本番環境用のクライアント。
    pub fn new() -> Self {
        Self::with_base("https://api.github.com".to_string())
    }

    /// テスト用にエンドポイントの基点を差し替える（末尾の `/` は不要）。
    pub fn with_base(base: String) -> Self {
        Self {
            client: http::client(),
            base: base.trim_end_matches('/').to_string(),
            page_size: DEFAULT_PAGE_SIZE,
            max_pages: DEFAULT_MAX_PAGES,
        }
    }

    /// テスト用にページングの大きさを変える。
    pub fn with_paging(mut self, page_size: usize, max_pages: u32) -> Self {
        self.page_size = page_size.max(1);
        self.max_pages = max_pages.max(1);
        self
    }

    /// GET して JSON を読む。404 は `Ok(None)`、それ以外の失敗は分類した `AuthError`。
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        token: &AccessToken,
        path_and_query: &str,
    ) -> Result<Option<T>, AuthError> {
        let url = format!("{}{}", self.base, path_and_query);
        let response = http::with_github_headers(self.client.get(url), token)
            .send()
            .await
            .map_err(http::classify_send_error)?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            let remaining = http::rate_limit_remaining(&response);
            return Err(http::classify_status(status, remaining.as_deref()));
        }
        response
            .json::<T>()
            .await
            .map(Some)
            .map_err(|_| AuthError::JsonError)
    }

    /// 保存先の一覧（個人 + 所属 Organization）。
    ///
    /// Organization は `GET /orgs/{org}/memberships/{login}` で参加状態を確認し、
    /// 作成権限がないものは `can_create=false` と理由を付ける（設計書 4.6）。
    /// 確認できない場合は作成できる側に倒し、実際の作成時の 403 で E02 として扱う。
    pub async fn list_owners(&self, token: &AccessToken) -> Result<Vec<Owner>, AuthError> {
        let user = UserClient::with_endpoint(format!("{}/user", self.base))
            .fetch_user(token)
            .await?;

        let mut owners = vec![Owner {
            login: user.login.clone(),
            kind: OwnerKind::Personal,
            avatar_url: user.avatar_url.clone(),
            can_create: true,
            block_reason: None,
        }];

        let mut orgs: Vec<OrgSummary> = Vec::new();
        for page in 1..=self.max_pages {
            let batch: Vec<OrgSummary> = self
                .get_json(
                    token,
                    &format!("/user/orgs?per_page={}&page={}", self.page_size, page),
                )
                .await?
                .unwrap_or_default();
            let count = batch.len();
            orgs.extend(batch);
            if count < self.page_size {
                break;
            }
        }
        orgs.sort_by_key(|o| o.login.to_ascii_lowercase());

        for org in orgs {
            let (can_create, block_reason) =
                self.org_creation(token, &org.login, &user.login).await?;
            owners.push(Owner {
                login: org.login,
                kind: OwnerKind::Org,
                avatar_url: org.avatar_url,
                can_create,
                block_reason,
            });
        }
        Ok(owners)
    }

    /// Organization にリポジトリを作れるか。
    async fn org_creation(
        &self,
        token: &AccessToken,
        org: &str,
        login: &str,
    ) -> Result<(bool, Option<CreateBlockReason>), AuthError> {
        let membership = self
            .get_json_tolerating_denial::<Membership>(
                token,
                &format!("/orgs/{org}/memberships/{login}"),
            )
            .await?;
        let Some(membership) = membership else {
            return Ok((false, Some(CreateBlockReason::NoAccess)));
        };
        if membership.state != "active" {
            return Ok((false, Some(CreateBlockReason::PendingInvitation)));
        }
        if membership.role == "admin" {
            return Ok((true, None));
        }
        // 一般メンバーは Organization の設定次第。設定を読めない場合は作成できる側に倒す
        let detail = self
            .get_json_tolerating_denial::<OrgDetail>(token, &format!("/orgs/{org}"))
            .await?;
        if detail.and_then(|d| d.members_can_create_repositories) == Some(false) {
            return Ok((false, Some(CreateBlockReason::MembersCannotCreate)));
        }
        Ok((true, None))
    }

    /// `get_json` のうち、403 と 404 を「情報なし」として扱う版（Organization ごとの確認用）。
    async fn get_json_tolerating_denial<T: serde::de::DeserializeOwned>(
        &self,
        token: &AccessToken,
        path_and_query: &str,
    ) -> Result<Option<T>, AuthError> {
        match self.get_json(token, path_and_query).await {
            Err(AuthError::Forbidden) => Ok(None),
            other => other,
        }
    }

    /// 自分が取得できるリポジトリの一覧（所有 + 所属 Organization）。
    /// オーナー（個人が先、Organization は名前順）ごとにまとまり、各オーナー内は名前順に並べる。
    /// `query` が空でなければ、`owner/name` に含まれる（大文字小文字を区別しない）ものだけに絞る。
    pub async fn list_repos(
        &self,
        token: &AccessToken,
        query: &str,
    ) -> Result<RepoList, AuthError> {
        let me = UserClient::with_endpoint(format!("{}/user", self.base))
            .fetch_user(token)
            .await?
            .login;

        let mut all: Vec<RepoResponse> = Vec::new();
        let mut truncated = false;
        for page in 1..=self.max_pages {
            let batch: Vec<RepoResponse> = self
                .get_json(
                    token,
                    &format!(
                        "/user/repos?affiliation=owner,organization_member&sort=full_name&per_page={}&page={}",
                        self.page_size, page
                    ),
                )
                .await?
                .unwrap_or_default();
            let count = batch.len();
            all.extend(batch);
            if count < self.page_size {
                break;
            }
            if page == self.max_pages {
                truncated = true;
            }
        }

        let repos = group_and_filter(
            all.into_iter()
                .map(|r| RemoteRepo {
                    full_name: r.full_name,
                    name: r.name,
                    owner_login: r.owner.login,
                    private: r.private,
                    updated_at: r.pushed_at.or(r.updated_at),
                })
                .collect(),
            &me,
            query,
        );
        Ok(RepoList { repos, truncated })
    }

    /// 同名の既存リポジトリに、新規作成の代わりに接続してよいかを調べる（設計書 4.6）。読み取りのみ。
    ///
    /// 次をすべて満たすときだけ `Adoptable`:
    /// 1. `GET /repos/{owner}/{name}` で見え、自分に書き込み権限がある（`permissions`。アーカイブ済みや
    ///    無効化されたものは不可）
    /// 2. 保存が 1 つも無い空である（`GET /repos/{owner}/{name}/commits?per_page=1` が 409
    ///    `Git Repository is empty`、または 200 で空の配列かつ大きさが 0）
    ///
    /// 判断に迷うもの（想定外の応答、権限の情報が無い）は接続しない側に倒す。
    /// 応答本文・URL・トークンはエラーにも結果にも含めない。
    pub async fn check_adoptable_repository(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
    ) -> Result<RepositoryAdoption, AuthError> {
        validate_owner_login(owner)?;
        validate_repository_name(name)?;

        let detail = match self
            .get_json::<RepoDetail>(token, &format!("/repos/{owner}/{name}"))
            .await
        {
            Ok(Some(detail)) => detail,
            Ok(None) => return Ok(RepositoryAdoption::NotFound),
            // 権限が無い（Organization の制限など）。書き込めないものとして扱う
            Err(AuthError::Forbidden) => return Ok(RepositoryAdoption::NoWriteAccess),
            Err(e) => return Err(e),
        };
        let can_write = detail
            .permissions
            .as_ref()
            .is_some_and(|p| p.admin || p.maintain || p.push)
            && !detail.archived
            && !detail.disabled;
        if !can_write {
            return Ok(RepositoryAdoption::NoWriteAccess);
        }

        match self
            .probe_empty(token, owner, name, detail.size == 0)
            .await?
        {
            EmptyProbe::Empty => Ok(RepositoryAdoption::Adoptable(RemoteRepo {
                full_name: detail.full_name,
                name: detail.name,
                owner_login: detail.owner.login,
                private: detail.private,
                updated_at: detail.pushed_at.or(detail.updated_at),
            })),
            EmptyProbe::NotEmpty => Ok(RepositoryAdoption::NotEmpty),
            EmptyProbe::NotFound => Ok(RepositoryAdoption::NotFound),
            EmptyProbe::NoAccess => Ok(RepositoryAdoption::NoWriteAccess),
        }
    }

    /// 保存（commit）が 1 つも無いかを調べる。`size_is_zero` は 200 で空の配列が返ったときの補助判定。
    async fn probe_empty(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        size_is_zero: bool,
    ) -> Result<EmptyProbe, AuthError> {
        let url = format!("{}/repos/{owner}/{name}/commits?per_page=1", self.base);
        let response = http::with_github_headers(self.client.get(url), token)
            .send()
            .await
            .map_err(http::classify_send_error)?;
        let status = response.status();
        match status {
            StatusCode::CONFLICT => {
                // 空のリポジトリは 409「Git Repository is empty」。それ以外の 409 は空と見なさない
                #[derive(Deserialize)]
                struct Body {
                    #[serde(default)]
                    message: Option<String>,
                }
                let empty = response
                    .json::<Body>()
                    .await
                    .ok()
                    .and_then(|b| b.message)
                    .is_some_and(|m| m.to_ascii_lowercase().contains("empty"));
                Ok(if empty {
                    EmptyProbe::Empty
                } else {
                    EmptyProbe::NotEmpty
                })
            }
            StatusCode::NOT_FOUND => Ok(EmptyProbe::NotFound),
            s if s.is_success() => {
                let commits = response
                    .json::<Vec<serde_json::Value>>()
                    .await
                    .map_err(|_| AuthError::JsonError)?;
                Ok(if commits.is_empty() && size_is_zero {
                    EmptyProbe::Empty
                } else {
                    EmptyProbe::NotEmpty
                })
            }
            _ => {
                let remaining = http::rate_limit_remaining(&response);
                match http::classify_status(status, remaining.as_deref()) {
                    // 保存の一覧を読めない（権限なし）。書き込めないものとして扱う
                    AuthError::Forbidden => Ok(EmptyProbe::NoAccess),
                    other => Err(other),
                }
            }
        }
    }

    /// 新しいリポジトリを作る（設計書 4.6）。個人は `POST /user/repos`、Organization は
    /// `POST /orgs/{org}/repos`。`private` を呼び出し側が必ず決める（既定は非公開にすること）。
    ///
    /// - 所有者名とリポジトリ名は送信前に検証する（URL の経路に載る所有者名の注入を防ぐ）
    /// - 同名が既にある（422）は `RepositoryNameTaken`、権限が無い（403・Organization が見えない 404）は
    ///   `Forbidden`（E02）。レート制限・通信不可・GitHub 側の障害は他の API と同じ分類
    /// - 応答本文・URL・トークンはエラーに含めない。作成後の URL は呼び出し側が `clone_url_for` で組み立てる
    pub async fn create_repository(
        &self,
        token: &AccessToken,
        owner: &str,
        kind: OwnerKind,
        name: &str,
        private: bool,
        description: Option<&str>,
    ) -> Result<RemoteRepo, AuthError> {
        validate_owner_login(owner)?;
        validate_repository_name(name)?;

        let path = match kind {
            OwnerKind::Personal => "/user/repos".to_string(),
            OwnerKind::Org => format!("/orgs/{owner}/repos"),
        };
        let mut body = serde_json::json!({
            "name": name,
            "private": private,
            "auto_init": false,
        });
        if let Some(text) = description.map(str::trim).filter(|d| !d.is_empty()) {
            body["description"] = serde_json::Value::String(text.to_string());
        }

        let url = format!("{}{}", self.base, path);
        let response = http::with_github_headers(self.client.post(url), token)
            .json(&body)
            .send()
            .await
            .map_err(http::classify_send_error)?;

        let status = response.status();
        if status == StatusCode::UNPROCESSABLE_ENTITY {
            return Err(classify_unprocessable(response).await);
        }
        // Organization が見えない（存在しない、または OAuth アプリの利用が許可されていない）
        if status == StatusCode::NOT_FOUND {
            return Err(AuthError::Forbidden);
        }
        if !status.is_success() {
            let remaining = http::rate_limit_remaining(&response);
            return Err(http::classify_status(status, remaining.as_deref()));
        }

        let created = response
            .json::<RepoResponse>()
            .await
            .map_err(|_| AuthError::JsonError)?;
        Ok(RemoteRepo {
            full_name: created.full_name,
            name: created.name,
            owner_login: created.owner.login,
            private: created.private,
            updated_at: created.pushed_at.or(created.updated_at),
        })
    }
}

/// 422 の応答から、同名の衝突とそれ以外の入力エラーを見分ける。応答本文は返さず分類だけにする。
async fn classify_unprocessable(response: reqwest::Response) -> AuthError {
    #[derive(Deserialize)]
    struct Detail {
        #[serde(default)]
        field: Option<String>,
        #[serde(default)]
        message: Option<String>,
    }
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        errors: Vec<Detail>,
    }
    let Ok(body) = response.json::<Body>().await else {
        return AuthError::UnexpectedStatus;
    };
    let exists = body.errors.iter().any(|e| {
        e.message
            .as_deref()
            .is_some_and(|m| m.to_ascii_lowercase().contains("already exists"))
    });
    if exists {
        return AuthError::RepositoryNameTaken;
    }
    if body
        .errors
        .iter()
        .any(|e| e.field.as_deref() == Some("name"))
    {
        return AuthError::InvalidRepositoryName;
    }
    AuthError::UnexpectedStatus
}

/// GitHub のアカウント名・Organization 名として使える文字だけか（英数字と `-`、39 文字以内）。
/// API の経路（`/orgs/{org}/repos`）に載せる値のため、`/` や `..` が混ざらないことを保証する。
pub fn validate_owner_login(owner: &str) -> Result<(), AuthError> {
    let valid = !owner.is_empty()
        && owner.len() <= 39
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AuthError::InvalidOwner)
    }
}

/// リポジトリ名の長さの上限（GitHub の規則）
const MAX_REPOSITORY_NAME_LEN: usize = 100;

/// リポジトリ名の検証。英数字と `-`・`_`・`.` だけ、1〜100 文字。
/// `.` と `..`、`.git` で終わる名前（GitHub は末尾を取り除いて別名にする）と、`.wiki` で終わる名前
/// （Wiki 用のリポジトリ名と衝突する）は予約名として拒否する。
pub fn validate_repository_name(name: &str) -> Result<(), AuthError> {
    let lower = name.to_ascii_lowercase();
    let valid = !name.is_empty()
        && name.len() <= MAX_REPOSITORY_NAME_LEN
        && name != "."
        && name != ".."
        && !lower.ends_with(".git")
        && !lower.ends_with(".wiki")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(AuthError::InvalidRepositoryName)
    }
}

/// プロジェクトの表示名から、GitHub で使えるリポジトリ名の案を作る。
/// ASCII の英数字と `-`・`_`・`.` だけを残し、空白は `-` にする。日本語の名前などで何も残らない
/// ときは `hikae-<識別子の先頭 8 文字>` にする（`seed` はプロジェクト ID）。結果は必ず検証を通る。
pub fn suggest_repository_name(display_name: &str, seed: &str) -> String {
    let mut out = String::new();
    for c in display_name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.') {
            out.push(c);
        } else if c == '-' || c.is_whitespace() {
            // 連続する区切りは 1 つにまとめる
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    let mut name: String = trimmed.chars().take(MAX_REPOSITORY_NAME_LEN).collect();
    if validate_repository_name(&name).is_err() {
        let short: String = seed
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(8)
            .collect::<String>()
            .to_ascii_lowercase();
        name = if short.is_empty() {
            "hikae-project".to_string()
        } else {
            format!("hikae-{short}")
        };
    }
    name
}

impl Default for GithubApi {
    fn default() -> Self {
        Self::new()
    }
}

/// 検索語で絞り込み、オーナーごとにまとめて並べる（個人が先、他は名前順。各グループ内も名前順）。
fn group_and_filter(mut repos: Vec<RemoteRepo>, me: &str, query: &str) -> Vec<RemoteRepo> {
    let needle = query.trim().to_lowercase();
    if !needle.is_empty() {
        repos.retain(|r| r.full_name.to_lowercase().contains(&needle));
    }
    repos.sort_by(|a, b| {
        let rank = |r: &RemoteRepo| u8::from(!r.owner_login.eq_ignore_ascii_case(me));
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                a.owner_login
                    .to_lowercase()
                    .cmp(&b.owner_login.to_lowercase())
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    repos
}

/// `owner/name` 形式のリポジトリ名を検証し、https の clone URL を作る。
/// GitHub の命名規則（英数字・`-`・`_`・`.`）に合わないものは None。
/// URL は常に github.com 固定で、任意の URL を受け付けない。
pub fn clone_url_for(full_name: &str) -> Option<String> {
    let (owner, name) = full_name.split_once('/')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.len() <= 100
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (valid(owner) && valid(name)).then(|| format!("https://github.com/{owner}/{name}.git"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn token() -> AccessToken {
        AccessToken::new("test_token_value".to_string())
    }

    fn mock_user(server: &MockServer) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/user")
                .header("authorization", "Bearer test_token_value");
            then.status(200).json_body(serde_json::json!({
                "id": 1, "login": "alice", "avatar_url": "https://a.example/alice.png"
            }));
        });
    }

    #[tokio::test]
    async fn owners_include_personal_and_mark_orgs_that_cannot_create() {
        let server = MockServer::start();
        mock_user(&server);
        server.mock(|when, then| {
            when.method(GET).path("/user/orgs");
            then.status(200).json_body(serde_json::json!([
                {"login": "zeta-admin", "avatar_url": "https://a.example/z.png"},
                {"login": "beta-member-open"},
                {"login": "gamma-member-closed"},
                {"login": "delta-pending"},
                {"login": "epsilon-hidden"},
            ]));
        });
        let membership = |org: &str, state: &str, role: &str| {
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/orgs/{org}/memberships/alice"));
                then.status(200)
                    .json_body(serde_json::json!({"state": state, "role": role}));
            });
        };
        membership("zeta-admin", "active", "admin");
        membership("beta-member-open", "active", "member");
        membership("gamma-member-closed", "active", "member");
        membership("delta-pending", "pending", "member");
        server.mock(|when, then| {
            when.method(GET)
                .path("/orgs/epsilon-hidden/memberships/alice");
            then.status(404);
        });
        server.mock(|when, then| {
            when.method(GET).path("/orgs/beta-member-open");
            then.status(200)
                .json_body(serde_json::json!({"members_can_create_repositories": true}));
        });
        server.mock(|when, then| {
            when.method(GET).path("/orgs/gamma-member-closed");
            then.status(200)
                .json_body(serde_json::json!({"members_can_create_repositories": false}));
        });

        let api = GithubApi::with_base(server.url(""));
        let owners = api.list_owners(&token()).await.expect("owners");

        let by = |login: &str| owners.iter().find(|o| o.login == login).expect(login);
        assert_eq!(owners[0].login, "alice");
        assert_eq!(owners[0].kind, OwnerKind::Personal);
        assert!(owners[0].can_create);
        assert_eq!(
            owners[0].avatar_url.as_deref(),
            Some("https://a.example/alice.png")
        );
        // 個人の次に Organization が名前順に並ぶ
        let order: Vec<&str> = owners.iter().map(|o| o.login.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "alice",
                "beta-member-open",
                "delta-pending",
                "epsilon-hidden",
                "gamma-member-closed",
                "zeta-admin"
            ]
        );
        assert!(by("zeta-admin").can_create);
        assert!(by("beta-member-open").can_create);
        assert!(!by("gamma-member-closed").can_create);
        assert_eq!(
            by("gamma-member-closed").block_reason,
            Some(CreateBlockReason::MembersCannotCreate)
        );
        assert_eq!(
            by("delta-pending").block_reason,
            Some(CreateBlockReason::PendingInvitation)
        );
        assert_eq!(
            by("epsilon-hidden").block_reason,
            Some(CreateBlockReason::NoAccess)
        );
        assert_eq!(by("zeta-admin").kind, OwnerKind::Org);
    }

    #[tokio::test]
    async fn org_settings_denied_falls_back_to_allowing_creation() {
        let server = MockServer::start();
        mock_user(&server);
        server.mock(|when, then| {
            when.method(GET).path("/user/orgs");
            then.status(200)
                .json_body(serde_json::json!([{"login": "acme"}]));
        });
        server.mock(|when, then| {
            when.method(GET).path("/orgs/acme/memberships/alice");
            then.status(200)
                .json_body(serde_json::json!({"state": "active", "role": "member"}));
        });
        server.mock(|when, then| {
            when.method(GET).path("/orgs/acme");
            then.status(403);
        });
        let api = GithubApi::with_base(server.url(""));
        let owners = api.list_owners(&token()).await.expect("owners");
        assert!(owners[1].can_create);
        assert_eq!(owners[1].block_reason, None);
    }

    #[tokio::test]
    async fn repos_are_paged_grouped_and_filtered() {
        let server = MockServer::start();
        mock_user(&server);
        let repo = |owner: &str, name: &str| {
            serde_json::json!({
                "name": name,
                "full_name": format!("{owner}/{name}"),
                "private": true,
                "owner": {"login": owner},
                "pushed_at": "2026-01-02T03:04:05Z",
            })
        };
        // ページサイズ 2: 1 ページ目は満杯、2 ページ目は 1 件で終わる
        server.mock(|when, then| {
            when.method(GET)
                .path("/user/repos")
                .query_param("page", "1")
                .query_param("per_page", "2")
                .query_param("affiliation", "owner,organization_member");
            then.status(200).json_body(serde_json::json!([
                repo("acme", "Report"),
                repo("alice", "thesis")
            ]));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/user/repos")
                .query_param("page", "2");
            then.status(200)
                .json_body(serde_json::json!([repo("alice", "Notes")]));
        });

        let api = GithubApi::with_base(server.url("")).with_paging(2, 5);
        let all = api.list_repos(&token(), "").await.expect("repos");
        assert!(!all.truncated);
        let names: Vec<&str> = all.repos.iter().map(|r| r.full_name.as_str()).collect();
        // 個人（alice）が先、次に Organization。各グループ内は名前順
        assert_eq!(names, vec!["alice/Notes", "alice/thesis", "acme/Report"]);
        assert_eq!(
            all.repos[0].updated_at.as_deref(),
            Some("2026-01-02T03:04:05Z")
        );

        let filtered = api.list_repos(&token(), " REPORT ").await.expect("repos");
        assert_eq!(filtered.repos.len(), 1);
        assert_eq!(filtered.repos[0].full_name, "acme/Report");
    }

    #[tokio::test]
    async fn repos_report_truncation_at_the_page_limit() {
        let server = MockServer::start();
        mock_user(&server);
        server.mock(|when, then| {
            when.method(GET).path("/user/repos");
            then.status(200).json_body(serde_json::json!([
                {"name": "a", "full_name": "alice/a", "private": false, "owner": {"login": "alice"}},
                {"name": "b", "full_name": "alice/b", "private": false, "owner": {"login": "alice"}},
            ]));
        });
        let api = GithubApi::with_base(server.url("")).with_paging(2, 3);
        let list = api.list_repos(&token(), "").await.expect("repos");
        assert!(list.truncated);
        assert_eq!(list.repos.len(), 6);
    }

    #[tokio::test]
    async fn http_failures_are_classified_without_leaking_the_token() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/user");
            then.status(401);
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api.list_owners(&token()).await.expect_err("401");
        assert!(matches!(err, AuthError::Unauthorized));
        assert!(!format!("{err} {err:?}").contains("test_token_value"));

        let server = MockServer::start();
        mock_user(&server);
        server.mock(|when, then| {
            when.method(GET).path("/user/repos");
            then.status(503);
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api.list_repos(&token(), "").await.expect_err("503");
        assert!(matches!(err, AuthError::ServerUnavailable));

        let server = MockServer::start();
        mock_user(&server);
        server.mock(|when, then| {
            when.method(GET).path("/user/repos");
            then.status(403).header("x-ratelimit-remaining", "0");
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api.list_repos(&token(), "").await.expect_err("429");
        assert!(matches!(err, AuthError::RateLimited));
    }

    #[tokio::test]
    async fn unreachable_server_is_reported_as_network_unavailable() {
        // 何も待ち受けていないローカルポートへ接続する
        let api = GithubApi::with_base("http://127.0.0.1:1".to_string());
        let err = api.list_owners(&token()).await.expect_err("unreachable");
        assert!(matches!(err, AuthError::NetworkUnavailable));
        assert!(!format!("{err} {err:?}").contains("test_token_value"));
    }

    #[test]
    fn clone_url_accepts_only_plain_owner_and_name() {
        assert_eq!(
            clone_url_for("alice/thesis").as_deref(),
            Some("https://github.com/alice/thesis.git")
        );
        assert_eq!(
            clone_url_for("my-org/report_2026.v2").as_deref(),
            Some("https://github.com/my-org/report_2026.v2.git")
        );
        for bad in [
            "",
            "alice",
            "alice/",
            "/thesis",
            "a/b/c",
            "alice/../x",
            "../thesis",
            "alice/th esis",
            "https://evil.example/x/y",
            "alice/thesis?x=1",
            "user:pw@host/x",
        ] {
            assert_eq!(clone_url_for(bad), None, "{bad}");
        }
    }

    fn created_body(owner: &str, name: &str, private: bool) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "full_name": format!("{owner}/{name}"),
            "private": private,
            "owner": {"login": owner},
            "updated_at": "2026-10-08T00:00:00Z",
        })
    }

    #[tokio::test]
    async fn personal_repository_is_created_private_through_user_repos() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/user/repos")
                .header("authorization", "Bearer test_token_value")
                .header("user-agent", "Hikae")
                .json_body(serde_json::json!({
                    "name": "thesis",
                    "private": true,
                    "auto_init": false,
                    "description": "卒業論文の控え",
                }));
            then.status(201)
                .json_body(created_body("alice", "thesis", true));
        });
        let api = GithubApi::with_base(server.url(""));
        let repo = api
            .create_repository(
                &token(),
                "alice",
                OwnerKind::Personal,
                "thesis",
                true,
                Some(" 卒業論文の控え "),
            )
            .await
            .expect("created");
        mock.assert();
        assert_eq!(repo.full_name, "alice/thesis");
        assert!(repo.private);
        assert_eq!(
            clone_url_for(&repo.full_name).as_deref(),
            Some("https://github.com/alice/thesis.git")
        );
    }

    #[tokio::test]
    async fn organization_repository_uses_the_org_endpoint_and_can_be_public() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/orgs/acme-team/repos")
                .json_body(serde_json::json!({
                    "name": "report",
                    "private": false,
                    "auto_init": false,
                }));
            then.status(201)
                .json_body(created_body("acme-team", "report", false));
        });
        let api = GithubApi::with_base(server.url(""));
        let repo = api
            .create_repository(&token(), "acme-team", OwnerKind::Org, "report", false, None)
            .await
            .expect("created");
        mock.assert();
        assert!(!repo.private);
        assert_eq!(repo.owner_login, "acme-team");
    }

    #[tokio::test]
    async fn existing_name_is_a_dedicated_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/user/repos");
            then.status(422).json_body(serde_json::json!({
                "message": "Repository creation failed.",
                "errors": [{
                    "resource": "Repository",
                    "code": "custom",
                    "field": "name",
                    "message": "name already exists on this account"
                }]
            }));
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api
            .create_repository(&token(), "alice", OwnerKind::Personal, "thesis", true, None)
            .await
            .expect_err("422");
        assert!(matches!(err, AuthError::RepositoryNameTaken));
        assert!(!format!("{err} {err:?}").contains("test_token_value"));
    }

    #[tokio::test]
    async fn other_unprocessable_responses_are_classified_without_the_body() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/user/repos")
                .json_body(serde_json::json!({
                    "name": "badname", "private": true, "auto_init": false
                }));
            then.status(422).json_body(serde_json::json!({
                "message": "Repository creation failed.",
                "errors": [{"resource": "Repository", "code": "invalid", "field": "name"}]
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/user/repos")
                .json_body(serde_json::json!({
                    "name": "weird", "private": true, "auto_init": false
                }));
            then.status(422).body("not json");
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api
            .create_repository(
                &token(),
                "alice",
                OwnerKind::Personal,
                "badname",
                true,
                None,
            )
            .await
            .expect_err("422");
        assert!(matches!(err, AuthError::InvalidRepositoryName));
        let err = api
            .create_repository(&token(), "alice", OwnerKind::Personal, "weird", true, None)
            .await
            .expect_err("422");
        assert!(matches!(err, AuthError::UnexpectedStatus));
    }

    #[tokio::test]
    async fn creation_failures_follow_the_existing_classification() {
        type Check = fn(&AuthError) -> bool;
        let cases: [(u16, Option<&str>, Check); 6] = [
            (401, None, |e| matches!(e, AuthError::Unauthorized)),
            // Organization の管理者の許可が必要（E02）
            (403, None, |e| matches!(e, AuthError::Forbidden)),
            (403, Some("0"), |e| matches!(e, AuthError::RateLimited)),
            (429, None, |e| matches!(e, AuthError::RateLimited)),
            // Organization が見えない
            (404, None, |e| matches!(e, AuthError::Forbidden)),
            (503, None, |e| matches!(e, AuthError::ServerUnavailable)),
        ];
        for (status, remaining, check) in cases {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(POST).path("/orgs/acme/repos");
                let then = then.status(status);
                let _ = match remaining {
                    Some(value) => then.header("x-ratelimit-remaining", value),
                    None => then,
                };
            });
            let api = GithubApi::with_base(server.url(""));
            let err = api
                .create_repository(&token(), "acme", OwnerKind::Org, "report", true, None)
                .await
                .expect_err("failure");
            assert!(check(&err), "{status}: {err:?}");
            assert!(!format!("{err} {err:?}").contains("test_token_value"));
        }
    }

    #[tokio::test]
    async fn creation_to_an_unreachable_server_is_network_unavailable() {
        let api = GithubApi::with_base("http://127.0.0.1:1".to_string());
        let err = api
            .create_repository(&token(), "alice", OwnerKind::Personal, "thesis", true, None)
            .await
            .expect_err("unreachable");
        assert!(matches!(err, AuthError::NetworkUnavailable));
        assert!(!format!("{err} {err:?}").contains("test_token_value"));
    }

    #[tokio::test]
    async fn invalid_names_are_rejected_before_any_request() {
        // 何も待ち受けていないポート。検証より先に通信すると NetworkUnavailable になる
        let api = GithubApi::with_base("http://127.0.0.1:1".to_string());
        for bad in [
            "",
            ".",
            "..",
            "a b",
            "卒業論文",
            "a/b",
            "x.git",
            "x.WIKI",
            &"a".repeat(101),
        ] {
            let err = api
                .create_repository(&token(), "alice", OwnerKind::Personal, bad, true, None)
                .await
                .expect_err(bad);
            assert!(matches!(err, AuthError::InvalidRepositoryName), "{bad}");
        }
        for bad in ["", "../x", "a/b", "-a", "a-", "a_b", "a b", &"a".repeat(40)] {
            let err = api
                .create_repository(&token(), bad, OwnerKind::Org, "ok", true, None)
                .await
                .expect_err(bad);
            assert!(matches!(err, AuthError::InvalidOwner), "{bad}");
        }
    }

    #[test]
    fn valid_repository_names_are_accepted() {
        for ok in [
            "thesis",
            "my-report_2026.v2",
            ".github",
            "A",
            &"a".repeat(100),
        ] {
            assert!(validate_repository_name(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn suggested_names_are_always_valid_and_ascii() {
        assert_eq!(
            suggest_repository_name("My Report 2026", "id"),
            "My-Report-2026"
        );
        assert_eq!(suggest_repository_name("  a  --  b  ", "id"), "a-b");
        assert_eq!(suggest_repository_name("report_v1.2", "id"), "report_v1.2");
        // 日本語だけで何も残らないときは、識別子から作る
        assert_eq!(
            suggest_repository_name("卒業論文", "1A2B-3c4d-5e6f"),
            "hikae-1a2b3c4d"
        );
        assert_eq!(suggest_repository_name("卒業論文", ""), "hikae-project");
        // 予約名になってしまう候補は使わない
        assert_eq!(
            suggest_repository_name("notes.git", "abcd1234"),
            "hikae-abcd1234"
        );
        assert_eq!(suggest_repository_name("..", "abcd1234"), "hikae-abcd1234");
        for input in ["", "日本語 English", &"x".repeat(300), "a/b:c*d", "-.-"] {
            let name = suggest_repository_name(input, "seed-0001");
            assert!(validate_repository_name(&name).is_ok(), "{input} -> {name}");
        }
    }

    /// 既存リポジトリの応答。`push` は書き込み権限、`size` は KB
    fn detail_body(owner: &str, name: &str, push: bool, size: u64) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "full_name": format!("{owner}/{name}"),
            "private": true,
            "owner": {"login": owner},
            "size": size,
            "archived": false,
            "permissions": {"admin": push, "maintain": push, "push": push, "pull": true},
            "updated_at": "2026-10-08T00:00:00Z",
        })
    }

    fn mock_detail(server: &MockServer, body: serde_json::Value) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/alice/thesis")
                .header("authorization", "Bearer test_token_value");
            then.status(200).json_body(body);
        });
    }

    #[tokio::test]
    async fn an_empty_writable_repository_can_be_adopted() {
        let server = MockServer::start();
        mock_detail(&server, detail_body("alice", "thesis", true, 0));
        let commits = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/alice/thesis/commits")
                .query_param("per_page", "1");
            then.status(409)
                .json_body(serde_json::json!({"message": "Git Repository is empty."}));
        });
        let api = GithubApi::with_base(server.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        commits.assert();
        let RepositoryAdoption::Adoptable(repo) = result else {
            panic!("expected adoptable, got {result:?}");
        };
        assert_eq!(repo.full_name, "alice/thesis");
        assert!(repo.private);
    }

    #[tokio::test]
    async fn an_empty_array_counts_as_empty_only_when_the_size_is_zero() {
        for (size, expect_adoptable) in [(0, true), (12, false)] {
            let server = MockServer::start();
            mock_detail(&server, detail_body("alice", "thesis", true, size));
            server.mock(|when, then| {
                when.method(GET).path("/repos/alice/thesis/commits");
                then.status(200).json_body(serde_json::json!([]));
            });
            let api = GithubApi::with_base(server.url(""));
            let result = api
                .check_adoptable_repository(&token(), "alice", "thesis")
                .await
                .expect("check");
            assert_eq!(
                matches!(result, RepositoryAdoption::Adoptable(_)),
                expect_adoptable,
                "size {size}: {result:?}"
            );
            if !expect_adoptable {
                assert_eq!(result, RepositoryAdoption::NotEmpty);
            }
        }
    }

    #[tokio::test]
    async fn a_repository_with_commits_is_never_adoptable() {
        let server = MockServer::start();
        mock_detail(&server, detail_body("alice", "thesis", true, 0));
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis/commits");
            then.status(200)
                .json_body(serde_json::json!([{"sha": "0123456789abcdef"}]));
        });
        let api = GithubApi::with_base(server.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NotEmpty);
    }

    #[tokio::test]
    async fn a_conflict_that_is_not_about_emptiness_is_not_adoptable() {
        let server = MockServer::start();
        mock_detail(&server, detail_body("alice", "thesis", true, 0));
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis/commits");
            then.status(409)
                .json_body(serde_json::json!({"message": "Something else"}));
        });
        let api = GithubApi::with_base(server.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NotEmpty);
    }

    #[tokio::test]
    async fn a_repository_without_write_access_is_not_adoptable() {
        // 読み取り専用。保存の有無は確認しない（commits は呼ばれない）
        let server2 = MockServer::start();
        mock_detail(&server2, detail_body("alice", "thesis", false, 0));
        let commits = server2.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis/commits");
            then.status(409)
                .json_body(serde_json::json!({"message": "Git Repository is empty."}));
        });
        let api = GithubApi::with_base(server2.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NoWriteAccess);
        assert_eq!(commits.calls(), 0);

        // 権限の情報が無い応答も接続しない側に倒す
        let server3 = MockServer::start();
        mock_detail(
            &server3,
            serde_json::json!({
                "name": "thesis", "full_name": "alice/thesis", "private": true,
                "owner": {"login": "alice"}, "size": 0
            }),
        );
        let api = GithubApi::with_base(server3.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NoWriteAccess);

        // アーカイブ済みは書き込めない
        let server4 = MockServer::start();
        let mut archived = detail_body("alice", "thesis", true, 0);
        archived["archived"] = serde_json::json!(true);
        mock_detail(&server4, archived);
        let api = GithubApi::with_base(server4.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NoWriteAccess);

        // 詳細の取得が 403（Organization の制限など）
        let server5 = MockServer::start();
        server5.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis");
            then.status(403);
        });
        let api = GithubApi::with_base(server5.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NoWriteAccess);
    }

    #[tokio::test]
    async fn a_missing_repository_is_reported_as_not_found() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis");
            then.status(404);
        });
        let api = GithubApi::with_base(server.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NotFound);

        // 詳細は見えるが、保存の一覧が 404
        let server = MockServer::start();
        mock_detail(&server, detail_body("alice", "thesis", true, 0));
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis/commits");
            then.status(404);
        });
        let api = GithubApi::with_base(server.url(""));
        let result = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect("check");
        assert_eq!(result, RepositoryAdoption::NotFound);
    }

    #[tokio::test]
    async fn adoption_check_failures_are_classified_without_the_token() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis");
            then.status(401);
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect_err("401");
        assert!(matches!(err, AuthError::Unauthorized));
        assert!(!format!("{err} {err:?}").contains("test_token_value"));

        let server = MockServer::start();
        mock_detail(&server, detail_body("alice", "thesis", true, 0));
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/thesis/commits");
            then.status(503);
        });
        let api = GithubApi::with_base(server.url(""));
        let err = api
            .check_adoptable_repository(&token(), "alice", "thesis")
            .await
            .expect_err("503");
        assert!(matches!(err, AuthError::ServerUnavailable));

        // 名前の検証は通信より先に行う
        let api = GithubApi::with_base("http://127.0.0.1:1".to_string());
        let err = api
            .check_adoptable_repository(&token(), "alice", "../x")
            .await
            .expect_err("invalid");
        assert!(matches!(err, AuthError::InvalidRepositoryName));
        let err = api
            .check_adoptable_repository(&token(), "../x", "thesis")
            .await
            .expect_err("invalid");
        assert!(matches!(err, AuthError::InvalidOwner));
    }
}
