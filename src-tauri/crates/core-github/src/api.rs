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
}
