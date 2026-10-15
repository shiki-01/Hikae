// 閲覧（読み取り）の操作（設計書 14.4）。

use crate::allow::Call;
use crate::client::{parse, RemoteClient};
use crate::encode::is_object_id;
use crate::error::RemoteError;
use crate::model::{DirEntry, DirListing, EntryKind, RepoSummary, VersionInfo, MAX_FILE_BYTES};
use crate::path::{check_list_path, check_repo};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use core_github::AccessToken;
use serde::Deserialize;
use serde_json::Value;

/// 保存先の一覧の 1 ページの件数（GitHub の上限）
const LIST_PAGE_SIZE: usize = 100;
/// 保存先の一覧の取得ページ数の上限（これを超える分は表示しない）
const LIST_MAX_PAGES: u32 = 5;
/// GitHub の contents API が 1 フォルダで返す件数の上限
const CONTENTS_LIMIT: usize = 1000;
/// 過去の版の取得件数
const HISTORY_LIMIT: usize = 30;

/// `GET /repos/{o}/{n}` と `GET /user/repos` の 1 件
#[derive(Deserialize)]
struct RepoResponse {
    name: String,
    owner: OwnerResponse,
    private: bool,
    default_branch: String,
    pushed_at: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct OwnerResponse {
    login: String,
}

impl From<RepoResponse> for RepoSummary {
    fn from(r: RepoResponse) -> Self {
        RepoSummary {
            owner: r.owner.login,
            name: r.name,
            private: r.private,
            default_branch: r.default_branch,
            pushed_at: r.pushed_at,
            description: r.description,
        }
    }
}

/// `GET .../git/ref/heads/{b}` の応答
#[derive(Deserialize)]
struct RefResponse {
    object: ObjectRef,
}

#[derive(Deserialize)]
struct ObjectRef {
    sha: String,
}

/// `GET .../git/commits/{sha}` の応答のうち、tree の ID
#[derive(Deserialize)]
struct CommitTreeResponse {
    tree: ObjectRef,
}

/// `GET .../contents/{p}` の配列の 1 件
#[derive(Deserialize)]
struct ContentResponse {
    name: String,
    path: String,
    sha: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: u64,
}

/// フォルダの中身の 1 件（種類は文字列のまま。同名の確認では種類を問わず見る）
#[derive(Debug, Clone)]
pub(crate) struct RawEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub sha: String,
    pub size: u64,
}

/// フォルダの中身の取得結果
#[derive(Debug)]
pub(crate) struct Listing {
    pub entries: Vec<RawEntry>,
    /// 1,000 件の上限に達しており、全部は見えていない可能性がある
    pub truncated: bool,
}

/// contents API の応答が、フォルダ（配列）かファイル（オブジェクト）か
#[derive(Debug)]
pub(crate) enum Folder {
    Listed(Listing),
    File,
}

/// `GET .../git/blobs/{sha}` の応答
#[derive(Deserialize)]
struct BlobResponse {
    size: u64,
    content: String,
    encoding: String,
}

/// `GET .../commits` の 1 件
#[derive(Deserialize)]
struct CommitResponse {
    sha: String,
    commit: CommitBody,
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
    author: Option<PersonResponse>,
}

#[derive(Deserialize)]
struct PersonResponse {
    name: Option<String>,
    date: Option<String>,
}

impl RemoteClient {
    /// 保存先の一覧（自分が所有・共同作業・Organization のメンバーとして見えるもの）。
    /// 更新順に並ぶ。最大 5 ページ（500 件）まで取得する。
    pub async fn list_repos(&self, token: &AccessToken) -> Result<Vec<RepoSummary>, RemoteError> {
        let mut repos = Vec::new();
        for page in 1..=LIST_MAX_PAGES {
            let page_text = page.to_string();
            let value = self
                .send(
                    token,
                    Call::ListRepos,
                    &[
                        ("per_page", "100"),
                        ("sort", "pushed"),
                        ("affiliation", "owner,collaborator,organization_member"),
                        ("page", page_text.as_str()),
                    ],
                    None,
                )
                .await?;
            let batch: Vec<RepoResponse> = parse(value)?;
            let count = batch.len();
            repos.extend(batch.into_iter().map(RepoSummary::from));
            if count < LIST_PAGE_SIZE {
                break;
            }
        }
        Ok(repos)
    }

    /// 1 つのリポジトリの要約（既定ブランチの確認にも使う）。
    pub async fn repo_summary(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
    ) -> Result<RepoSummary, RemoteError> {
        check_repo(owner, name)?;
        let value = self
            .send(token, Call::RepoSummary { owner, name }, &[], None)
            .await?;
        let repo: RepoResponse = parse(value)?;
        Ok(repo.into())
    }

    /// フォルダの一覧。`git_ref` が None のときは既定ブランチの先頭を使う。
    /// `git_ref` を指定したとき（過去の版の閲覧）は、その ID をそのまま基準にする。
    /// フォルダを先に、名前順（大文字小文字を区別しない）で並べる。
    pub async fn list_dir(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        path: &str,
        git_ref: Option<&str>,
    ) -> Result<DirListing, RemoteError> {
        check_repo(owner, name)?;
        let path = check_list_path(path)?;

        let (branch, head_sha) = match git_ref {
            Some(sha) => {
                if !is_object_id(sha) {
                    return Err(RemoteError::InvalidPath(
                        "版の ID の形式が正しくありません".to_string(),
                    ));
                }
                (sha.to_string(), sha.to_string())
            }
            None => {
                let branch = self.repo_summary(token, owner, name).await?.default_branch;
                let head_sha = self.branch_head(token, owner, name, &branch).await?;
                (branch, head_sha)
            }
        };

        let listing = match self.folder(token, owner, name, &path, &head_sha).await? {
            Folder::File => return Err(RemoteError::NotFound),
            Folder::Listed(listing) => listing,
        };

        let mut entries: Vec<DirEntry> = listing
            .entries
            .into_iter()
            .filter_map(|e| {
                let kind = match e.kind.as_str() {
                    "file" => EntryKind::File,
                    "dir" => EntryKind::Dir,
                    // サブモジュール・シンボリックリンクは表示しない
                    _ => return None,
                };
                Some(DirEntry {
                    name: e.name,
                    path: e.path,
                    kind,
                    size: e.size,
                    sha: e.sha,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            let a_dir = a.kind == EntryKind::Dir;
            let b_dir = b.kind == EntryKind::Dir;
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(DirListing {
            branch,
            head_sha,
            truncated: listing.truncated,
            entries,
        })
    }

    /// ファイルの中身を取得する。`size_hint` が上限を超えるときは通信しない。
    /// 応答の大きさも上限を超えるときは `TooLarge`（取得した内容は捨てる）。
    pub async fn fetch_blob(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        sha: &str,
        size_hint: Option<u64>,
    ) -> Result<Vec<u8>, RemoteError> {
        check_repo(owner, name)?;
        if !is_object_id(sha) {
            return Err(RemoteError::InvalidPath(
                "ファイルの ID の形式が正しくありません".to_string(),
            ));
        }
        if size_hint.is_some_and(|s| s > MAX_FILE_BYTES) {
            return Err(RemoteError::TooLarge);
        }
        let value = self
            .send(token, Call::GitBlob { owner, name, sha }, &[], None)
            .await?;
        let blob: BlobResponse = parse(value)?;
        if blob.size > MAX_FILE_BYTES {
            return Err(RemoteError::TooLarge);
        }
        if blob.encoding != "base64" {
            return Err(RemoteError::InvalidResponse);
        }
        // GitHub は base64 を 60 桁ごとに改行して返すので、空白を除いてからデコードする
        let cleaned: String = blob
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = STANDARD
            .decode(cleaned)
            .map_err(|_| RemoteError::InvalidResponse)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(RemoteError::TooLarge);
        }
        Ok(bytes)
    }

    /// ファイルの過去の版（新しい順。最大 30 件）。既定ブランチの履歴を見る。
    pub async fn file_history(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        path: &str,
    ) -> Result<Vec<VersionInfo>, RemoteError> {
        check_repo(owner, name)?;
        let path = check_list_path(path)?;
        let limit = HISTORY_LIMIT.to_string();
        let mut query: Vec<(&str, &str)> = Vec::new();
        if !path.is_empty() {
            query.push(("path", path.as_str()));
        }
        query.push(("per_page", limit.as_str()));
        let value = self
            .send(token, Call::Commits { owner, name }, &query, None)
            .await?;
        let commits: Vec<CommitResponse> = parse(value)?;
        Ok(commits
            .into_iter()
            .map(|c| {
                let author = c.commit.author;
                VersionInfo {
                    sha: c.sha,
                    message: c
                        .commit
                        .message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim_end()
                        .to_string(),
                    date: author
                        .as_ref()
                        .and_then(|a| a.date.clone())
                        .unwrap_or_default(),
                    author: author.and_then(|a| a.name).unwrap_or_default(),
                }
            })
            .collect())
    }

    /// 指定した基準（コミットの ID）の時点のファイルの blob の ID とバイト数。
    /// 過去の版を開くために使う。ファイルでなければ `NotFound`。
    pub async fn file_at(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<(String, u64), RemoteError> {
        check_repo(owner, name)?;
        let path = check_list_path(path)?;
        if path.is_empty() || !is_object_id(git_ref) {
            return Err(RemoteError::InvalidPath(
                "基準の指定が正しくありません".to_string(),
            ));
        }
        let value = self
            .send(
                token,
                Call::Contents {
                    owner,
                    name,
                    path: &path,
                },
                &[("ref", git_ref)],
                None,
            )
            .await?;
        let Value::Object(_) = value else {
            return Err(RemoteError::NotFound);
        };
        let entry: ContentResponse = parse(value)?;
        if entry.kind != "file" || !is_object_id(&entry.sha) {
            return Err(RemoteError::NotFound);
        }
        Ok((entry.sha, entry.size))
    }

    /// 指定したブランチの先頭コミットの ID。空のリポジトリ（ref が無い）では `NotFound`。
    pub(crate) async fn branch_head(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> Result<String, RemoteError> {
        let value = self
            .send(
                token,
                Call::GitRef {
                    owner,
                    name,
                    branch,
                },
                &[],
                None,
            )
            .await?;
        let reference: RefResponse = parse(value)?;
        if !is_object_id(&reference.object.sha) {
            return Err(RemoteError::InvalidResponse);
        }
        Ok(reference.object.sha)
    }

    /// コミットのツリーの ID。
    pub(crate) async fn commit_tree(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        commit: &str,
    ) -> Result<String, RemoteError> {
        let value = self
            .send(
                token,
                Call::GitCommit {
                    owner,
                    name,
                    sha: commit,
                },
                &[],
                None,
            )
            .await?;
        let commit: CommitTreeResponse = parse(value)?;
        if !is_object_id(&commit.tree.sha) {
            return Err(RemoteError::InvalidResponse);
        }
        Ok(commit.tree.sha)
    }

    /// 指定した基準の時点のフォルダの中身。フォルダが無ければ `NotFound`、ファイルなら `Folder::File`。
    /// `path` は検査済みであること（空はルート）。
    pub(crate) async fn folder(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        path: &str,
        head_sha: &str,
    ) -> Result<Folder, RemoteError> {
        let value = self
            .send(
                token,
                Call::Contents { owner, name, path },
                &[("ref", head_sha)],
                None,
            )
            .await?;
        match value {
            Value::Array(items) => {
                let truncated = items.len() >= CONTENTS_LIMIT;
                let parsed: Vec<ContentResponse> = parse(Value::Array(items))?;
                let entries = parsed
                    .into_iter()
                    .map(|c| RawEntry {
                        name: c.name,
                        path: c.path,
                        kind: c.kind,
                        sha: c.sha,
                        size: c.size,
                    })
                    .collect();
                Ok(Folder::Listed(Listing { entries, truncated }))
            }
            Value::Object(_) => Ok(Folder::File),
            _ => Err(RemoteError::InvalidResponse),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_response_maps_to_summary() {
        let value = serde_json::json!({
            "name": "notes",
            "owner": {"login": "alice"},
            "private": true,
            "default_branch": "main",
            "pushed_at": "2026-10-01T00:00:00Z",
            "description": null,
            "extra": 1
        });
        let repo: RepoResponse = parse(value).expect("parse");
        let summary = RepoSummary::from(repo);
        assert_eq!(summary.owner, "alice");
        assert_eq!(summary.name, "notes");
        assert!(summary.private);
        assert_eq!(summary.default_branch, "main");
        assert_eq!(summary.pushed_at.as_deref(), Some("2026-10-01T00:00:00Z"));
        assert_eq!(summary.description, None);
    }
}
