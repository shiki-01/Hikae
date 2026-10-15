// アップロードの統合テスト（httpmock）。GitHub の Git Data API の呼び出しと本文を検証する。

mod common;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use common::*;
use core_remote::{
    OnExists, PlanItem, RemoteError, UploadItem, UploadRequest, MAX_FILE_BYTES, MAX_ITEMS,
};
use httpmock::prelude::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 記録された POST・PATCH の本文
struct Recorded {
    blobs: Bodies,
    trees: Bodies,
    commits: Bodies,
    patches: Bodies,
}

fn count(bodies: &Bodies) -> usize {
    bodies.lock().expect("lock").len()
}

fn last(bodies: &Bodies) -> Value {
    bodies
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .unwrap_or(Value::Null)
}

fn nth(bodies: &Bodies, i: usize) -> Value {
    bodies
        .lock()
        .expect("lock")
        .get(i)
        .cloned()
        .unwrap_or(Value::Null)
}

/// n 回目の blob の ID（テストの mock が決めた値）
fn blob_sha(n: u64) -> String {
    format!("{:040x}", 0xb000 + n)
}

/// 同じ名前のフォルダの一覧（`ref` は HEAD）。`None` は 404（フォルダが無い）。
fn list_mock(server: &MockServer, dir: &str, listing: Option<Value>) {
    let path = if dir.is_empty() {
        "/repos/alice/notes/contents".to_string()
    } else {
        format!("/repos/alice/notes/contents/{dir}")
    };
    server.mock(|when, then| {
        when.method(GET).path(path).query_param("ref", HEAD);
        match listing {
            Some(value) => then.status(200).json_body(value),
            None => then.status(404),
        };
    });
}

/// blob の作成。呼ぶたびに別の ID を返し、本文を記録する。
fn blob_responder(
    bodies: Bodies,
) -> impl Fn(&HttpMockRequest) -> HttpMockResponse + Send + Sync + 'static {
    let counter = Arc::new(AtomicUsize::new(0));
    move |req: &HttpMockRequest| {
        let body: Value = serde_json::from_slice(req.body_ref()).unwrap_or(Value::Null);
        bodies.lock().expect("lock").push(body);
        let n = counter.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        json_response(201, &json!({ "sha": blob_sha(n) }))
    }
}

/// PATCH の応答を順番に返す（最後の値を繰り返す）。本文は記録する。
fn patch_responder(
    bodies: Bodies,
    statuses: Vec<u16>,
) -> impl Fn(&HttpMockRequest) -> HttpMockResponse + Send + Sync + 'static {
    let counter = Arc::new(AtomicUsize::new(0));
    move |req: &HttpMockRequest| {
        let body: Value = serde_json::from_slice(req.body_ref()).unwrap_or(Value::Null);
        bodies.lock().expect("lock").push(body);
        let n = counter.fetch_add(1, Ordering::SeqCst);
        let status = statuses.get(n).or(statuses.last()).copied().unwrap_or(200);
        json_response(status, &json!({"ref": "refs/heads/main"}))
    }
}

/// 既定ブランチ（main）の基本の mock と、フォルダの一覧、書き込みの mock を置く。
fn setup(
    server: &MockServer,
    listings: Vec<(&str, Option<Value>)>,
    patch_statuses: Vec<u16>,
) -> Recorded {
    base_mocks(server, "alice", "notes");
    for (dir, listing) in listings {
        list_mock(server, dir, listing);
    }
    let rec = Recorded {
        blobs: bodies(),
        trees: bodies(),
        commits: bodies(),
        patches: bodies(),
    };
    server.mock(|when, then| {
        when.method(POST).path("/repos/alice/notes/git/blobs");
        then.respond_with(blob_responder(rec.blobs.clone()));
    });
    server.mock(|when, then| {
        when.method(POST).path("/repos/alice/notes/git/trees");
        then.respond_with(recording(rec.trees.clone(), 201, json!({"sha": NEW_TREE})));
    });
    server.mock(|when, then| {
        when.method(POST).path("/repos/alice/notes/git/commits");
        then.respond_with(recording(
            rec.commits.clone(),
            201,
            json!({"sha": NEW_COMMIT}),
        ));
    });
    server.mock(|when, then| {
        when.method(PATCH)
            .path("/repos/alice/notes/git/refs/heads/main");
        then.respond_with(patch_responder(rec.patches.clone(), patch_statuses));
    });
    rec
}

fn item(dir: &str, name: &str, content: &[u8], on_exists: OnExists) -> UploadItem {
    UploadItem {
        target_dir: dir.to_string(),
        name: name.to_string(),
        content: content.to_vec(),
        on_exists,
    }
}

fn request(items: Vec<UploadItem>) -> UploadRequest {
    UploadRequest {
        message: None,
        items,
    }
}

#[tokio::test]
async fn upload_builds_blob_tree_commit_and_fast_forwards_the_ref() {
    let server = MockServer::start();
    let rec = setup(&server, vec![("photos", None)], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("photos", "a.jpg", b"abc", OnExists::Skip)]),
        )
        .await
        .expect("upload");

    assert_eq!(result.commit_sha, NEW_COMMIT);
    assert_eq!(result.saved, vec!["photos/a.jpg".to_string()]);
    assert!(result.skipped.is_empty());
    assert!(result.renamed.is_empty());

    // blob: base64 で送る
    assert_eq!(count(&rec.blobs), 1);
    let blob = last(&rec.blobs);
    assert_eq!(blob["encoding"], "base64");
    assert_eq!(blob["content"], STANDARD.encode(b"abc"));

    // tree: HEAD のツリーの上に、通常ファイルとして載せる
    assert_eq!(count(&rec.trees), 1);
    let tree = last(&rec.trees);
    assert_eq!(tree["base_tree"], TREE);
    let entries = tree["tree"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["path"], "photos/a.jpg");
    assert_eq!(entries[0]["mode"], "100644");
    assert_eq!(entries[0]["type"], "blob");
    assert_eq!(entries[0]["sha"], blob_sha(1));

    // commit: 親は HEAD の 1 つだけ。作者と committer は noreply アドレス
    assert_eq!(count(&rec.commits), 1);
    let commit = last(&rec.commits);
    assert_eq!(commit["parents"], json!([HEAD]));
    assert_eq!(commit["tree"], NEW_TREE);
    assert_eq!(commit["message"], "スマホから追加（1 件）");
    let email = "12345+alice@users.noreply.github.com";
    assert_eq!(commit["author"], json!({"name": "alice", "email": email}));
    assert_eq!(
        commit["committer"],
        json!({"name": "alice", "email": email})
    );

    // ref: fast-forward のみ（force は false）
    assert_eq!(count(&rec.patches), 1);
    let patch = last(&rec.patches);
    assert_eq!(patch["sha"], NEW_COMMIT);
    assert_eq!(patch["force"], Value::Bool(false));
}

#[tokio::test]
async fn same_name_choices_replace_keep_both_and_skip() {
    let server = MockServer::start();
    let existing = json!([
        content("Notes.TXT", "file", "e1"),
        content("a.txt", "file", "e2"),
        content("B.txt", "file", "e3"),
    ]);
    let rec = setup(&server, vec![("", Some(existing))], vec![200]);

    let mut req = request(vec![
        item("", "notes.txt", b"n", OnExists::Replace),
        item("", "a.txt", b"a", OnExists::KeepBoth),
        item("", "b.txt", b"b", OnExists::Skip),
        item("", "c.txt", b"c", OnExists::Skip),
    ]);
    req.message = Some("メモ".to_string());
    let result = client(&server)
        .upload(&token(), &user(), "alice", "notes", req)
        .await
        .expect("upload");

    // 置き換えは既存の表記で、両方残すは既存と重ならない名前で、追加しないは既存の表記で報告する
    assert_eq!(result.saved, vec!["Notes.TXT", "a (2).txt", "c.txt"]);
    assert_eq!(result.skipped, vec!["B.txt"]);
    assert_eq!(
        result.renamed,
        vec![core_remote::Renamed {
            from: "a.txt".to_string(),
            to: "a (2).txt".to_string(),
        }]
    );

    // 「追加しない」の b.txt は送らない（blob は 3 件。保存は 3 件）
    assert_eq!(count(&rec.blobs), 3);
    let paths: Vec<String> = last(&rec.trees)["tree"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["path"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(paths, vec!["Notes.TXT", "a (2).txt", "c.txt"]);
    assert_eq!(last(&rec.commits)["message"], "メモ");
}

#[tokio::test]
async fn a_folder_with_the_same_name_is_refused_before_writing() {
    let server = MockServer::start();
    let existing = json!([content("Docs", "dir", "d1")]);
    let rec = setup(&server, vec![("", Some(existing))], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("", "docs", b"x", OnExists::Replace)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::NameIsFolder(_))));
    assert_eq!(count(&rec.blobs), 0);
    assert_eq!(count(&rec.trees), 0);
    assert_eq!(count(&rec.patches), 0);
}

#[tokio::test]
async fn duplicate_targets_are_refused_before_writing() {
    let server = MockServer::start();
    let rec = setup(&server, vec![("", None)], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![
                item("", "a.txt", b"1", OnExists::Skip),
                item("", "A.TXT", b"2", OnExists::Skip),
            ]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::DuplicateTarget(p)) if p == "A.TXT"));
    assert_eq!(count(&rec.blobs), 0);
    assert_eq!(count(&rec.patches), 0);
}

#[tokio::test]
async fn a_422_is_retried_from_the_start_and_reuses_the_blob() {
    let server = MockServer::start();
    // 1 回目の ref の更新は他の端末に先を越された（422）。2 回目は成功する
    let rec = setup(&server, vec![("", None)], vec![422, 200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await
        .expect("upload after retry");

    assert_eq!(result.commit_sha, NEW_COMMIT);
    // blob は 1 回だけ作り、2 回目の試行で再利用する
    assert_eq!(count(&rec.blobs), 1);
    // tree と commit は作り直す。ref の更新は 2 回
    assert_eq!(count(&rec.trees), 2);
    assert_eq!(count(&rec.commits), 2);
    assert_eq!(count(&rec.patches), 2);
    for i in 0..2 {
        let patch = nth(&rec.patches, i);
        assert_eq!(patch["force"], Value::Bool(false));
    }
}

#[tokio::test]
async fn three_stale_attempts_give_up_without_moving_the_ref() {
    let server = MockServer::start();
    let rec = setup(&server, vec![("", None)], vec![422]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::NotFastForward)));
    // 3 回試して、どれも成功しなかった（ref は変わっていない）
    assert_eq!(count(&rec.patches), 3);
    assert_eq!(count(&rec.commits), 3);
    assert_eq!(count(&rec.blobs), 1);
}

#[tokio::test]
async fn a_non_422_failure_on_the_ref_is_not_retried() {
    let server = MockServer::start();
    let rec = setup(&server, vec![("", None)], vec![403]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::Auth(_))));
    assert_eq!(count(&rec.patches), 1);
}

#[tokio::test]
async fn when_every_item_is_skipped_nothing_is_written() {
    let server = MockServer::start();
    let existing = json!([content("a.txt", "file", "e1")]);
    let rec = setup(&server, vec![("", Some(existing))], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await
        .expect("upload");

    assert_eq!(result.commit_sha, "");
    assert!(result.saved.is_empty());
    assert_eq!(result.skipped, vec!["a.txt".to_string()]);
    assert_eq!(count(&rec.blobs), 0);
    assert_eq!(count(&rec.trees), 0);
    assert_eq!(count(&rec.commits), 0);
    assert_eq!(count(&rec.patches), 0);
}

#[tokio::test]
async fn a_missing_folder_is_created_by_the_tree() {
    let server = MockServer::start();
    let rec = setup(&server, vec![("new/deep", None)], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("/new/deep/", "a.txt", b"abc", OnExists::Replace)]),
        )
        .await
        .expect("upload");

    assert_eq!(result.saved, vec!["new/deep/a.txt".to_string()]);
    let entries = last(&rec.trees)["tree"].clone();
    assert_eq!(entries[0]["path"], "new/deep/a.txt");
}

#[tokio::test]
async fn an_empty_repository_is_not_found_and_nothing_is_created() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/alice/empty");
        then.status(200)
            .json_body(repo_json("alice", "empty", "main"));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/alice/empty/git/ref/heads/main");
        then.status(409);
    });
    let blob = server.mock(|when, then| {
        when.method(POST).path("/repos/alice/empty/git/blobs");
        then.status(201).json_body(json!({"sha": HEAD}));
    });

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "empty",
            request(vec![item("", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::NotFound)));
    blob.assert_calls(0);
}

#[tokio::test]
async fn a_folder_that_is_a_file_is_refused() {
    let server = MockServer::start();
    let rec = setup(
        &server,
        vec![(
            "docs",
            Some(json!({"name": "docs", "path": "docs", "sha": "f1", "size": 1, "type": "file"})),
        )],
        vec![200],
    );

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("docs", "a.txt", b"abc", OnExists::Skip)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::InvalidPath(_))));
    assert_eq!(count(&rec.blobs), 0);
}

#[tokio::test]
async fn a_folder_over_one_thousand_entries_is_refused_so_no_name_is_overwritten_blindly() {
    let server = MockServer::start();
    let big: Vec<Value> = (0..1000)
        .map(|i| content(&format!("f{i}.txt"), "file", "c1"))
        .collect();
    let rec = setup(&server, vec![("big", Some(json!(big)))], vec![200]);

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes",
            request(vec![item("big", "a.txt", b"abc", OnExists::Replace)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::TooMany)));
    assert_eq!(count(&rec.blobs), 0);
}

#[tokio::test]
async fn limits_and_paths_are_checked_before_any_request() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200);
    });
    let c = client(&server);

    // 件数
    assert!(matches!(
        try_upload(&c, vec![]).await,
        Err(RemoteError::EmptyRequest)
    ));
    let many: Vec<UploadItem> = (0..=MAX_ITEMS)
        .map(|i| item("", &format!("f{i}.txt"), b"x", OnExists::Skip))
        .collect();
    assert!(matches!(
        try_upload(&c, many).await,
        Err(RemoteError::TooMany)
    ));

    // 大きさ（1 件と合計）
    let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
    assert!(matches!(
        try_upload(&c, vec![item("", "big.bin", &big, OnExists::Skip)]).await,
        Err(RemoteError::TooLarge)
    ));
    let each = vec![0u8; MAX_FILE_BYTES as usize];
    let three = vec![
        item("", "1.bin", &each, OnExists::Skip),
        item("", "2.bin", &each, OnExists::Skip),
        item("", "3.bin", &each, OnExists::Skip),
    ];
    assert!(matches!(
        try_upload(&c, three).await,
        Err(RemoteError::TooLarge)
    ));

    // パス（区切り、予約名、.git、相対、長すぎる階層）
    assert!(matches!(
        try_upload(&c, vec![item("", "a/b.txt", b"x", OnExists::Skip)]).await,
        Err(RemoteError::InvalidPath(_))
    ));
    assert!(matches!(
        try_upload(&c, vec![item("", "CON.txt", b"x", OnExists::Skip)]).await,
        Err(RemoteError::InvalidPath(_))
    ));
    assert!(matches!(
        try_upload(&c, vec![item(".git", "config", b"x", OnExists::Skip)]).await,
        Err(RemoteError::InvalidPath(_))
    ));
    assert!(matches!(
        try_upload(&c, vec![item("../x", "a.txt", b"x", OnExists::Skip)]).await,
        Err(RemoteError::InvalidPath(_))
    ));
    let long_dir = (0..30).map(|_| "abcdefghi").collect::<Vec<_>>().join("/");
    assert!(matches!(
        try_upload(&c, vec![item(&long_dir, "a.txt", b"x", OnExists::Skip)]).await,
        Err(RemoteError::InvalidPath(_))
    ));

    any.assert_calls(0);
}

/// 既定のリポジトリ（alice/notes）への追加を 1 回試す。
async fn try_upload(
    c: &core_remote::RemoteClient,
    items: Vec<UploadItem>,
) -> Result<core_remote::UploadResult, RemoteError> {
    c.upload(&token(), &user(), "alice", "notes", request(items))
        .await
}

#[tokio::test]
async fn a_bad_repository_name_is_refused_before_any_request() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200);
    });

    let result = client(&server)
        .upload(
            &token(),
            &user(),
            "alice",
            "notes?x=1",
            request(vec![item("", "a.txt", b"x", OnExists::Skip)]),
        )
        .await;

    assert!(matches!(result, Err(RemoteError::InvalidPath(_))));
    any.assert_calls(0);
}

#[tokio::test]
async fn plan_reports_which_items_already_exist_and_writes_nothing() {
    let server = MockServer::start();
    let existing = json!([content("Notes.TXT", "file", "e1")]);
    setup(&server, vec![("", Some(existing))], vec![200]);

    let plan = client(&server)
        .plan_upload(
            &token(),
            "alice",
            "notes",
            &[
                PlanItem {
                    target_dir: String::new(),
                    name: "notes.txt".to_string(),
                    size: 3,
                },
                PlanItem {
                    target_dir: "/".to_string(),
                    name: "new.txt".to_string(),
                    size: 3,
                },
            ],
        )
        .await
        .expect("plan");

    assert_eq!(plan.branch, "main");
    assert_eq!(plan.items.len(), 2);
    assert_eq!(plan.items[0].target_path, "notes.txt");
    assert!(plan.items[0].exists);
    assert_eq!(plan.items[1].target_path, "new.txt");
    assert!(!plan.items[1].exists);
}

#[tokio::test]
async fn plan_refuses_a_folder_with_the_same_name() {
    let server = MockServer::start();
    let existing = json!([content("photos", "dir", "d1")]);
    setup(&server, vec![("", Some(existing))], vec![200]);

    let result = client(&server)
        .plan_upload(
            &token(),
            "alice",
            "notes",
            &[PlanItem {
                target_dir: String::new(),
                name: "Photos".to_string(),
                size: 1,
            }],
        )
        .await;

    assert!(matches!(result, Err(RemoteError::NameIsFolder(_))));
}
