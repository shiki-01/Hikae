// HTTP 呼び出しの許可リスト（設計書 14.5・CLAUDE.md の不変条件 1・5）。
//
// 送れる組み合わせは `Call` の列挙だけ。送信の直前に `is_allowed` と `is_allowed_body` で
// もう一度検査し、許可外なら `Disallowed` にする（二重の守り）。
// 許可するのは読み取りと、追加（blob・tree・commit の作成と、ref の fast-forward 更新）だけ。
// ref の作成・削除、タグ、force、強制更新は、どの形でも通さない。

use crate::encode::{encode_component, encode_path, is_object_id};
use reqwest::Method;
use serde_json::{json, Value};

/// 許可する呼び出しの一覧。値は呼び出し元が検査済みの owner・name・パス・ID。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Call<'a> {
    /// GET user/repos
    ListRepos,
    /// GET repos/{o}/{n}
    RepoSummary { owner: &'a str, name: &'a str },
    /// GET repos/{o}/{n}/contents/{p}（`path` は空でルート）
    Contents {
        owner: &'a str,
        name: &'a str,
        path: &'a str,
    },
    /// GET repos/{o}/{n}/git/ref/heads/{b}
    GitRef {
        owner: &'a str,
        name: &'a str,
        branch: &'a str,
    },
    /// GET repos/{o}/{n}/git/commits/{sha}
    GitCommit {
        owner: &'a str,
        name: &'a str,
        sha: &'a str,
    },
    /// GET repos/{o}/{n}/git/blobs/{sha}
    GitBlob {
        owner: &'a str,
        name: &'a str,
        sha: &'a str,
    },
    /// GET repos/{o}/{n}/commits
    Commits { owner: &'a str, name: &'a str },
    /// POST repos/{o}/{n}/git/blobs
    CreateBlob { owner: &'a str, name: &'a str },
    /// POST repos/{o}/{n}/git/trees
    CreateTree { owner: &'a str, name: &'a str },
    /// POST repos/{o}/{n}/git/commits
    CreateCommit { owner: &'a str, name: &'a str },
    /// PATCH repos/{o}/{n}/git/refs/heads/{b}（本文は `update_ref_body` だけが作る）
    UpdateRef {
        owner: &'a str,
        name: &'a str,
        branch: &'a str,
    },
}

impl Call<'_> {
    /// HTTP メソッド
    pub(crate) fn method(self) -> Method {
        match self {
            Call::ListRepos
            | Call::RepoSummary { .. }
            | Call::Contents { .. }
            | Call::GitRef { .. }
            | Call::GitCommit { .. }
            | Call::GitBlob { .. }
            | Call::Commits { .. } => Method::GET,
            Call::CreateBlob { .. } | Call::CreateTree { .. } | Call::CreateCommit { .. } => {
                Method::POST
            }
            Call::UpdateRef { .. } => Method::PATCH,
        }
    }

    /// クエリを除いたパス（先頭の `/` は付けない）。値はここでエンコードする。
    pub(crate) fn path(self) -> String {
        match self {
            Call::ListRepos => "user/repos".to_string(),
            Call::RepoSummary { owner, name } => format!("repos/{owner}/{name}"),
            Call::Contents { owner, name, path } => {
                if path.is_empty() {
                    format!("repos/{owner}/{name}/contents")
                } else {
                    format!("repos/{owner}/{name}/contents/{}", encode_path(path))
                }
            }
            Call::GitRef {
                owner,
                name,
                branch,
            } => format!("repos/{owner}/{name}/git/ref/heads/{}", encode_path(branch)),
            Call::GitCommit { owner, name, sha } => {
                format!("repos/{owner}/{name}/git/commits/{sha}")
            }
            Call::GitBlob { owner, name, sha } => format!("repos/{owner}/{name}/git/blobs/{sha}"),
            Call::Commits { owner, name } => format!("repos/{owner}/{name}/commits"),
            Call::CreateBlob { owner, name } => format!("repos/{owner}/{name}/git/blobs"),
            Call::CreateTree { owner, name } => format!("repos/{owner}/{name}/git/trees"),
            Call::CreateCommit { owner, name } => format!("repos/{owner}/{name}/git/commits"),
            Call::UpdateRef {
                owner,
                name,
                branch,
            } => format!(
                "repos/{owner}/{name}/git/refs/heads/{}",
                encode_path(branch)
            ),
        }
    }
}

/// クエリの値を作る（`k=v&...`）。値は必ずエンコードする。
pub(crate) fn query_string(query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return String::new();
    }
    let joined = query
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("?{joined}")
}

/// PATCH の本文。`force` は `false` に固定し、呼び出し側から変えられない。
pub(crate) fn update_ref_body(sha: &str) -> Value {
    json!({ "sha": sha, "force": false })
}

/// owner・name の形。エンコードしない値なので `%` を含めない。
fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// 1 階層（ブランチや内容のパスの 1 階層）が安全な形か。
/// エンコード後の値だけを想定し、`?` `#` `\` や空・`.`・`..` を通さない。
fn segment_ok(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'%'))
}

/// 複数の階層（ファイルのパスやブランチ名）。空の列は許すかどうかを呼び出し側で決める。
fn segments_ok(segments: &[&str]) -> bool {
    segments.iter().all(|s| segment_ok(s))
}

/// メソッドとパス（クエリを除く）が許可リストに入っているか。
pub(crate) fn is_allowed(method: &Method, path: &str) -> bool {
    let segs: Vec<&str> = path.split('/').collect();
    let get = method.as_str() == "GET";
    let post = method.as_str() == "POST";
    let patch = method.as_str() == "PATCH";
    match segs.as_slice() {
        ["user", "repos"] => get,
        ["repos", o, n] => get && name_ok(o) && name_ok(n),
        ["repos", o, n, "contents", rest @ ..] => {
            get && name_ok(o) && name_ok(n) && segments_ok(rest)
        }
        ["repos", o, n, "git", "ref", "heads", rest @ ..] => {
            get && name_ok(o) && name_ok(n) && !rest.is_empty() && segments_ok(rest)
        }
        ["repos", o, n, "git", "commits", sha] => {
            get && name_ok(o) && name_ok(n) && is_object_id(sha)
        }
        ["repos", o, n, "git", "blobs", sha] => {
            get && name_ok(o) && name_ok(n) && is_object_id(sha)
        }
        ["repos", o, n, "commits"] => get && name_ok(o) && name_ok(n),
        ["repos", o, n, "git", "blobs"]
        | ["repos", o, n, "git", "trees"]
        | ["repos", o, n, "git", "commits"] => post && name_ok(o) && name_ok(n),
        ["repos", o, n, "git", "refs", "heads", rest @ ..] => {
            patch && name_ok(o) && name_ok(n) && !rest.is_empty() && segments_ok(rest)
        }
        _ => false,
    }
}

/// 本文が許可された形か。GET は本文なし、POST は JSON オブジェクト、
/// PATCH は `{"sha": <ID>, "force": false}` の 2 項目だけ（`force` を真にする形は通さない）。
pub(crate) fn is_allowed_body(method: &Method, path: &str, body: Option<&Value>) -> bool {
    match method.as_str() {
        "GET" => body.is_none(),
        "POST" => matches!(body, Some(Value::Object(_))),
        "PATCH" => {
            let Some(Value::Object(map)) = body else {
                return false;
            };
            if map.len() != 2 {
                return false;
            }
            let sha_ok = matches!(map.get("sha"), Some(Value::String(s)) if is_object_id(s));
            let force_ok = matches!(map.get("force"), Some(Value::Bool(false)));
            // PATCH の対象は ref の更新だけ（is_allowed と同じ形）
            let path_ok = path.starts_with("repos/") && path.contains("/git/refs/heads/");
            sha_ok && force_ok && path_ok
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn every_constructible_call_is_allowed() {
        let calls = [
            Call::ListRepos,
            Call::RepoSummary {
                owner: "o",
                name: "n",
            },
            Call::Contents {
                owner: "o",
                name: "n",
                path: "",
            },
            Call::Contents {
                owner: "o",
                name: "n",
                path: "a b/c.txt",
            },
            Call::GitRef {
                owner: "o",
                name: "n",
                branch: "main",
            },
            Call::GitRef {
                owner: "o",
                name: "n",
                branch: "feature/x",
            },
            Call::GitCommit {
                owner: "o",
                name: "n",
                sha: SHA,
            },
            Call::GitBlob {
                owner: "o",
                name: "n",
                sha: SHA,
            },
            Call::Commits {
                owner: "o",
                name: "n",
            },
            Call::CreateBlob {
                owner: "o",
                name: "n",
            },
            Call::CreateTree {
                owner: "o",
                name: "n",
            },
            Call::CreateCommit {
                owner: "o",
                name: "n",
            },
            Call::UpdateRef {
                owner: "o",
                name: "n",
                branch: "main",
            },
        ];
        for call in calls {
            assert!(
                is_allowed(&call.method(), &call.path()),
                "{:?} {} must be allowed",
                call.method(),
                call.path()
            );
        }
    }

    #[test]
    fn delete_is_refused_on_any_path() {
        for path in [
            "repos/o/n",
            "repos/o/n/git/refs/heads/main",
            "repos/o/n/contents/a.txt",
            "repos/o/n/git/blobs",
        ] {
            assert!(!is_allowed(&Method::DELETE, path), "DELETE {path}");
        }
    }

    #[test]
    fn patch_is_allowed_only_for_branch_heads() {
        assert!(is_allowed(&Method::PATCH, "repos/o/n/git/refs/heads/main"));
        assert!(is_allowed(
            &Method::PATCH,
            "repos/o/n/git/refs/heads/feature/x"
        ));
        assert!(!is_allowed(&Method::PATCH, "repos/o/n/git/refs/tags/v1"));
        assert!(!is_allowed(&Method::PATCH, "repos/o/n/git/refs/heads"));
        assert!(!is_allowed(&Method::PATCH, "repos/o/n/git/refs"));
        assert!(!is_allowed(&Method::PATCH, "repos/o/n"));
        assert!(!is_allowed(
            &Method::PATCH,
            "repos/o/n/git/refs/heads/main/../x"
        ));
    }

    #[test]
    fn creating_refs_and_tags_is_refused() {
        assert!(!is_allowed(&Method::POST, "repos/o/n/git/refs"));
        assert!(!is_allowed(&Method::POST, "repos/o/n/git/refs/heads/new"));
        assert!(!is_allowed(&Method::POST, "repos/o/n/git/tags"));
        assert!(!is_allowed(&Method::POST, "repos/o/n/git/trees/extra"));
    }

    #[test]
    fn query_and_fragment_characters_are_refused_in_paths() {
        assert!(!is_allowed(&Method::GET, "repos/o/n/contents/a?ref=x"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/git/blobs/abc#x"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/contents/a\\b"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/contents/../x"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/contents//x"));
    }

    #[test]
    fn repo_segments_must_be_plain() {
        assert!(!is_allowed(&Method::GET, "repos/../n"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/x"));
        assert!(!is_allowed(&Method::GET, "repos/o%2Fx/n"));
        assert!(!is_allowed(&Method::GET, "user/orgs"));
    }

    #[test]
    fn git_objects_need_hex_ids() {
        assert!(!is_allowed(&Method::GET, "repos/o/n/git/blobs/main"));
        assert!(!is_allowed(&Method::GET, "repos/o/n/git/commits/..%2F"));
    }

    #[test]
    fn patch_body_must_be_exactly_sha_and_force_false() {
        let path = "repos/o/n/git/refs/heads/main";
        let ok = json!({"sha": SHA, "force": false});
        assert!(is_allowed_body(&Method::PATCH, path, Some(&ok)));

        let force_true = json!({"sha": SHA, "force": true});
        assert!(!is_allowed_body(&Method::PATCH, path, Some(&force_true)));

        let extra = json!({"sha": SHA, "force": false, "ref": "refs/heads/x"});
        assert!(!is_allowed_body(&Method::PATCH, path, Some(&extra)));

        let no_force = json!({"sha": SHA});
        assert!(!is_allowed_body(&Method::PATCH, path, Some(&no_force)));

        let bad_sha = json!({"sha": "main", "force": false});
        assert!(!is_allowed_body(&Method::PATCH, path, Some(&bad_sha)));

        assert!(!is_allowed_body(&Method::PATCH, path, None));
        assert!(!is_allowed_body(&Method::PATCH, "repos/o/n", Some(&ok)));
    }

    #[test]
    fn update_ref_body_is_fixed_to_force_false() {
        let body = update_ref_body(SHA);
        assert!(is_allowed_body(
            &Method::PATCH,
            "repos/o/n/git/refs/heads/main",
            Some(&body)
        ));
        assert_eq!(body["force"], Value::Bool(false));
    }

    #[test]
    fn get_must_not_carry_a_body_and_delete_is_never_allowed() {
        assert!(!is_allowed_body(
            &Method::GET,
            "user/repos",
            Some(&json!({}))
        ));
        assert!(is_allowed_body(&Method::GET, "user/repos", None));
        assert!(!is_allowed_body(&Method::DELETE, "repos/o/n", None));
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(
            query_string(&[("path", "a b/c&d"), ("per_page", "30")]),
            "?path=a%20b%2Fc%26d&per_page=30"
        );
        assert_eq!(query_string(&[]), "");
    }
}
