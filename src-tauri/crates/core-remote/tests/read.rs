// 閲覧（読み取り）の統合テスト（httpmock）。

mod common;

use common::*;
use core_github::AuthError;
use core_remote::{EntryKind, RemoteError, MAX_FILE_BYTES};
use httpmock::prelude::*;
use serde_json::json;

#[tokio::test]
async fn list_repos_sends_the_fixed_query_and_stops_at_a_short_page() {
    let server = MockServer::start();
    let page1 = server.mock(|when, then| {
        when.method(GET)
            .path("/user/repos")
            .query_param("per_page", "100")
            .query_param("sort", "pushed")
            .query_param("affiliation", "owner,collaborator,organization_member")
            .query_param("page", "1")
            .header("authorization", format!("Bearer {TOKEN}"));
        let items: Vec<_> = (0..100)
            .map(|i| repo_json("alice", &format!("repo-{i}"), "main"))
            .collect();
        then.status(200).json_body(json!(items));
    });
    let page2 = server.mock(|when, then| {
        when.method(GET)
            .path("/user/repos")
            .query_param("page", "2");
        then.status(200)
            .json_body(json!([repo_json("org", "shared", "trunk"),]));
    });
    let page3 = server.mock(|when, then| {
        when.method(GET)
            .path("/user/repos")
            .query_param("page", "3");
        then.status(200).json_body(json!([]));
    });

    let repos = client(&server).list_repos(&token()).await.expect("list");

    assert_eq!(repos.len(), 101);
    assert_eq!(repos[100].owner, "org");
    assert_eq!(repos[100].default_branch, "trunk");
    page1.assert_calls(1);
    page2.assert_calls(1);
    // 2 ページ目が 100 未満なので、3 ページ目は呼ばない
    page3.assert_calls(0);
}

#[tokio::test]
async fn list_repos_stops_after_five_full_pages() {
    let server = MockServer::start();
    let full = server.mock(|when, then| {
        when.method(GET).path("/user/repos");
        let items: Vec<_> = (0..100)
            .map(|i| repo_json("alice", &format!("r{i}"), "main"))
            .collect();
        then.status(200).json_body(json!(items));
    });

    let repos = client(&server).list_repos(&token()).await.expect("list");

    assert_eq!(repos.len(), 500);
    full.assert_calls(5);
}

#[tokio::test]
async fn repo_summary_maps_the_fields() {
    let server = MockServer::start();
    base_mocks(&server, "alice", "notes");

    let summary = client(&server)
        .repo_summary(&token(), "alice", "notes")
        .await
        .expect("summary");

    assert_eq!(summary.owner, "alice");
    assert_eq!(summary.name, "notes");
    assert!(summary.private);
    assert_eq!(summary.default_branch, "main");
    assert_eq!(summary.description.as_deref(), Some("memo"));
    assert_eq!(summary.pushed_at.as_deref(), Some("2026-10-01T00:00:00Z"));
}

#[tokio::test]
async fn list_dir_uses_the_default_branch_and_sorts_folders_first() {
    let server = MockServer::start();
    base_mocks(&server, "alice", "notes");
    let contents = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/alice/notes/contents/docs")
            .query_param("ref", HEAD);
        then.status(200).json_body(json!([
            content("b.md", "file", "a1"),
            content("zz", "dir", "a2"),
            content("link", "symlink", "a3"),
            content("sub", "submodule", "a4"),
            content("Aa", "dir", "a5"),
            content("a.txt", "file", "a6"),
        ]));
    });

    let listing = client(&server)
        .list_dir(&token(), "alice", "notes", "docs", None)
        .await
        .expect("list");

    assert_eq!(listing.branch, "main");
    assert_eq!(listing.head_sha, HEAD);
    assert!(!listing.truncated);
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Aa", "zz", "a.txt", "b.md"]);
    assert_eq!(listing.entries[0].kind, EntryKind::Dir);
    assert_eq!(listing.entries[2].kind, EntryKind::File);
    contents.assert();
}

#[tokio::test]
async fn list_dir_with_a_ref_uses_it_as_the_head_and_skips_the_lookups() {
    let server = MockServer::start();
    let repo = server.mock(|when, then| {
        when.method(GET).path("/repos/alice/notes");
        then.status(200)
            .json_body(repo_json("alice", "notes", "main"));
    });
    let contents = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/alice/notes/contents")
            .query_param("ref", NEW_COMMIT);
        then.status(200)
            .json_body(json!([content("old.txt", "file", "b1")]));
    });

    let listing = client(&server)
        .list_dir(&token(), "alice", "notes", "", Some(NEW_COMMIT))
        .await
        .expect("list");

    assert_eq!(listing.head_sha, NEW_COMMIT);
    assert_eq!(listing.entries.len(), 1);
    repo.assert_calls(0);
    contents.assert();
}

#[tokio::test]
async fn list_dir_marks_truncated_at_one_thousand_entries() {
    let server = MockServer::start();
    base_mocks(&server, "alice", "notes");
    server.mock(|when, then| {
        when.method(GET).path("/repos/alice/notes/contents/big");
        let items: Vec<_> = (0..1000)
            .map(|i| content(&format!("f{i}.txt"), "file", "c1"))
            .collect();
        then.status(200).json_body(json!(items));
    });

    let listing = client(&server)
        .list_dir(&token(), "alice", "notes", "big", None)
        .await
        .expect("list");

    assert!(listing.truncated);
    assert_eq!(listing.entries.len(), 1000);
}

#[tokio::test]
async fn list_dir_missing_folder_is_not_found() {
    let server = MockServer::start();
    base_mocks(&server, "alice", "notes");
    server.mock(|when, then| {
        when.method(GET).path("/repos/alice/notes/contents/nope");
        then.status(404);
    });

    let result = client(&server)
        .list_dir(&token(), "alice", "notes", "nope", None)
        .await;

    assert!(matches!(result, Err(RemoteError::NotFound)));
}

#[tokio::test]
async fn list_dir_on_a_file_is_not_found() {
    let server = MockServer::start();
    base_mocks(&server, "alice", "notes");
    server.mock(|when, then| {
        when.method(GET).path("/repos/alice/notes/contents/a.txt");
        then.status(200).json_body(json!({
            "name": "a.txt", "path": "a.txt", "sha": "d1", "size": 3, "type": "file"
        }));
    });

    let result = client(&server)
        .list_dir(&token(), "alice", "notes", "a.txt", None)
        .await;

    assert!(matches!(result, Err(RemoteError::NotFound)));
}

#[tokio::test]
async fn an_empty_repository_has_no_default_head_so_it_is_not_found() {
    // 空のリポジトリでは ref が無い（404 か 409）
    for status in [404u16, 409] {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/alice/empty");
            then.status(200)
                .json_body(repo_json("alice", "empty", "main"));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/alice/empty/git/ref/heads/main");
            then.status(status);
        });

        let result = client(&server)
            .list_dir(&token(), "alice", "empty", "", None)
            .await;

        assert!(
            matches!(result, Err(RemoteError::NotFound)),
            "status {status}"
        );
    }
}

#[tokio::test]
async fn list_dir_rejects_traversal_before_any_request() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200);
    });

    let result = client(&server)
        .list_dir(&token(), "alice", "notes", "docs/../secret", None)
        .await;

    assert!(matches!(result, Err(RemoteError::InvalidPath(_))));
    any.assert_calls(0);
}

#[tokio::test]
async fn fetch_blob_decodes_base64_with_line_breaks() {
    let server = MockServer::start();
    let blob = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/repos/alice/notes/git/blobs/{HEAD}"));
        then.status(200).json_body(json!({
            "size": 5,
            "encoding": "base64",
            "content": "aGVs\nbG8=\n",
        }));
    });

    let bytes = client(&server)
        .fetch_blob(&token(), "alice", "notes", HEAD, Some(5))
        .await
        .expect("blob");

    assert_eq!(bytes, b"hello");
    blob.assert();
}

#[tokio::test]
async fn fetch_blob_refuses_a_size_hint_over_the_limit_without_a_request() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200);
    });

    let result = client(&server)
        .fetch_blob(&token(), "alice", "notes", HEAD, Some(MAX_FILE_BYTES + 1))
        .await;

    assert!(matches!(result, Err(RemoteError::TooLarge)));
    any.assert_calls(0);
}

#[tokio::test]
async fn fetch_blob_refuses_a_reported_size_over_the_limit() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/repos/alice/notes/git/blobs/{HEAD}"));
        then.status(200).json_body(json!({
            "size": MAX_FILE_BYTES + 1,
            "encoding": "base64",
            "content": "",
        }));
    });

    let result = client(&server)
        .fetch_blob(&token(), "alice", "notes", HEAD, None)
        .await;

    assert!(matches!(result, Err(RemoteError::TooLarge)));
}

#[tokio::test]
async fn file_history_reads_the_first_line_date_and_author() {
    let server = MockServer::start();
    let commits = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/alice/notes/commits")
            .query_param("path", "docs/a.md")
            .query_param("per_page", "30");
        then.status(200).json_body(json!([
            {
                "sha": HEAD,
                "commit": {
                    "message": "スマホから追加（1 件）\n\n詳細",
                    "author": {"name": "alice", "date": "2026-10-02T03:04:05Z"}
                }
            },
            {
                "sha": TREE,
                "commit": {"message": "first", "author": null}
            }
        ]));
    });

    let history = client(&server)
        .file_history(&token(), "alice", "notes", "docs/a.md")
        .await
        .expect("history");

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].message, "スマホから追加（1 件）");
    assert_eq!(history[0].date, "2026-10-02T03:04:05Z");
    assert_eq!(history[0].author, "alice");
    assert_eq!(history[1].author, "");
    commits.assert();
}

#[tokio::test]
async fn auth_failures_are_classified_and_never_show_the_token() {
    type Check = fn(&RemoteError) -> bool;
    let cases: Vec<(u16, Option<&str>, Check)> = vec![
        (401, None, |e| {
            matches!(e, RemoteError::Auth(AuthError::Unauthorized))
        }),
        (403, Some("0"), |e| {
            matches!(e, RemoteError::Auth(AuthError::RateLimited))
        }),
        (403, None, |e| {
            matches!(e, RemoteError::Auth(AuthError::Forbidden))
        }),
        (503, None, |e| {
            matches!(e, RemoteError::Auth(AuthError::ServerUnavailable))
        }),
    ];
    for (status, remaining, check) in cases {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/user/repos");
            let mut reply = then.status(status);
            if let Some(value) = remaining {
                reply = reply.header("x-ratelimit-remaining", value);
            }
            reply.body("secret echo test_token_value");
        });

        let err = client(&server)
            .list_repos(&token())
            .await
            .expect_err("must fail");

        assert!(check(&err), "status {status}: {err:?}");
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains(TOKEN), "token leaked: {shown}");
    }
}

#[tokio::test]
async fn an_unreachable_server_is_reported_as_network_unavailable() {
    // 何も待ち受けていないポート
    let client = core_remote::RemoteClient::with_base("http://127.0.0.1:9");
    let result = client.list_repos(&token()).await;
    assert!(matches!(
        result,
        Err(RemoteError::Auth(AuthError::NetworkUnavailable))
    ));
}

#[tokio::test]
async fn file_at_returns_the_blob_of_that_commit() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/alice/notes/contents/docs/a.txt")
            .query_param("ref", HEAD);
        then.status(200).json_body(json!({
            "name": "a.txt", "path": "docs/a.txt", "type": "file",
            "sha": "5555555555555555555555555555555555555555", "size": 12
        }));
    });
    let (sha, size) = client(&server)
        .file_at(&token(), "alice", "notes", "docs/a.txt", HEAD)
        .await
        .expect("file_at");
    assert_eq!(sha, "5555555555555555555555555555555555555555");
    assert_eq!(size, 12);
    m.assert_calls(1);
}

#[tokio::test]
async fn file_at_refuses_folders_and_bad_refs_without_calling() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.method(GET);
        then.status(200).json_body(json!([]));
    });
    // フォルダ（配列）は NotFound
    let err = client(&server)
        .file_at(&token(), "alice", "notes", "docs", HEAD)
        .await
        .expect_err("folder");
    assert!(matches!(err, RemoteError::NotFound));
    any.assert_calls(1);
    // 基準が ID の形でないときは通信しない
    let err = client(&server)
        .file_at(&token(), "alice", "notes", "docs/a.txt", "main")
        .await
        .expect_err("bad ref");
    assert!(matches!(err, RemoteError::InvalidPath(_)));
    any.assert_calls(1);
}
