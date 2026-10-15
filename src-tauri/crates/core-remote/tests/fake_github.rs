// 状態を持つ疑似 GitHub サーバーを相手にした結合テスト（tests/fake/mod.rs）。
//
// httpmock（固定の応答）では確かめられない、複数回の操作・同時更新・履歴をまたいだ振る舞いを確かめる。
// 各テストの最後に、受けた全リクエストの規則違反が無いことを検査する。

mod fake;

use core_github::{AccessToken, User};
use core_remote::{
    DirListing, OnExists, PlanItem, RemoteClient, RemoteError, Renamed, UploadItem, UploadRequest,
    UploadResult,
};
use fake::*;

fn token() -> AccessToken {
    AccessToken::new(TOKEN_VALUE.to_string())
}

fn user() -> User {
    User::new(12345, "alice".to_string())
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

/// 決まった擬似乱数のバイト列（再現できるように seed から作る）
fn pseudo_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

/// フォルダの中身の名前（並びに依存しないよう、ソートして返す）
fn names(listing: &DirListing) -> Vec<String> {
    let mut v: Vec<String> = listing.entries.iter().map(|e| e.name.clone()).collect();
    v.sort();
    v
}

async fn list(client: &RemoteClient, dir: &str) -> DirListing {
    client
        .list_dir(&token(), OWNER, REPO, dir, None)
        .await
        .expect("list_dir")
}

/// フォルダの中の 1 ファイルを、一覧（list_dir）→ 取得（fetch_blob）の順に読む
async fn read_file(client: &RemoteClient, dir: &str, name: &str) -> Vec<u8> {
    let listing = list(client, dir).await;
    let entry = listing
        .entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{dir}/{name} が一覧に無い"));
    client
        .fetch_blob(&token(), OWNER, REPO, &entry.sha, Some(entry.size))
        .await
        .expect("fetch_blob")
}

async fn upload(
    client: &RemoteClient,
    items: Vec<UploadItem>,
) -> Result<UploadResult, RemoteError> {
    client
        .upload(&token(), &user(), OWNER, REPO, request(items))
        .await
}

// 1. 新規フォルダ・入れ子のフォルダへの追加

#[tokio::test]
async fn nested_new_folders_keep_bytes_and_leave_other_files_alone() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();
    let before_files = fake.tree_files(&before);

    let big = pseudo_bytes(3 * 1024 * 1024, 7);
    let all_bytes: Vec<u8> = (0..=255u8).collect();
    let items = vec![
        item(
            "新しい/深い",
            "メモ.txt",
            "こんにちは、世界\n".as_bytes(),
            OnExists::Skip,
        ),
        item("新しい/深い", "all.bin", &all_bytes, OnExists::Skip),
        item("新しい", "空.txt", b"", OnExists::Skip),
        item("新しい", "大きい.bin", &big, OnExists::Skip),
    ];
    let result = upload(&client, items).await.expect("upload");

    assert_eq!(
        result.saved,
        vec![
            "新しい/深い/メモ.txt",
            "新しい/深い/all.bin",
            "新しい/空.txt",
            "新しい/大きい.bin",
        ]
    );
    assert!(result.skipped.is_empty());
    assert!(result.renamed.is_empty());
    assert!(!result.commit_sha.is_empty());
    assert_eq!(
        fake.head(),
        result.commit_sha,
        "ref が新しい commit に進んでいる"
    );

    // コミットは 1 つの親（直前の先頭）を持ち、作者と committer は noreply アドレス
    let commit = fake.commit(&result.commit_sha).expect("commit");
    assert_eq!(commit.parents, vec![before.clone()]);
    assert_eq!(commit.author.name, "alice");
    assert_eq!(commit.author.email, ALICE_NOREPLY);
    assert_eq!(commit.committer.email, ALICE_NOREPLY);
    assert_eq!(commit.message, "スマホから追加（4 件）");

    // 既存のファイルは blob の ID まで変わらず、追加した 4 件だけが増える
    let after_files = fake.tree_files(&result.commit_sha);
    for (path, sha) in &before_files {
        assert_eq!(after_files.get(path), Some(sha), "{path} は変わらない");
    }
    let mut expected_paths: Vec<&str> = before_files.keys().map(String::as_str).collect();
    expected_paths.extend([
        "新しい/深い/メモ.txt",
        "新しい/深い/all.bin",
        "新しい/空.txt",
        "新しい/大きい.bin",
    ]);
    expected_paths.sort();
    let actual_paths: Vec<&str> = after_files.keys().map(String::as_str).collect();
    assert_eq!(actual_paths, expected_paths);

    // 一覧の階層が正しく見える
    let root = list(&client, "").await;
    assert_eq!(names(&root), vec!["README.md", "docs", "新しい", "資料"]);
    let nested = list(&client, "新しい/深い").await;
    assert_eq!(names(&nested), vec!["all.bin", "メモ.txt"]);
    let folder = list(&client, "新しい").await;
    // 名前は並びに依存しないようソートして比べる（UTF-8 のバイト順）
    assert_eq!(names(&folder), vec!["大きい.bin", "深い", "空.txt"]);
    let dir_entry = folder
        .entries
        .iter()
        .find(|e| e.name == "深い")
        .expect("深い");
    assert_eq!(dir_entry.kind, core_remote::EntryKind::Dir);

    // バイト列は完全に一致する（日本語名、バイナリ、空ファイル、3 MiB）
    assert_eq!(
        read_file(&client, "新しい/深い", "メモ.txt").await,
        "こんにちは、世界\n".as_bytes()
    );
    assert_eq!(
        read_file(&client, "新しい/深い", "all.bin").await,
        all_bytes
    );
    assert_eq!(
        read_file(&client, "新しい", "空.txt").await,
        Vec::<u8>::new()
    );
    assert_eq!(read_file(&client, "新しい", "大きい.bin").await, big);
    assert_eq!(read_file(&client, "", "README.md").await, b"# notes\n");
    assert_eq!(
        fake.file_bytes(&result.commit_sha, "新しい/大きい.bin"),
        Some(big)
    );

    assert_request_rules(&fake);
}

#[tokio::test]
async fn adding_into_an_existing_nested_folder_keeps_its_siblings() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();

    let result = upload(
        &client,
        vec![item(
            "資料",
            "追加.txt",
            "追加の内容".as_bytes(),
            OnExists::Skip,
        )],
    )
    .await
    .expect("upload");

    assert_eq!(result.saved, vec!["資料/追加.txt"]);
    assert_eq!(
        read_file(&client, "資料", "議事録.txt").await,
        "議事録の本文\n".as_bytes()
    );
    assert_eq!(
        read_file(&client, "資料", "追加.txt").await,
        "追加の内容".as_bytes()
    );
    assert_eq!(
        fake.commit(&result.commit_sha).expect("c").parents,
        vec![before]
    );

    assert_request_rules(&fake);
}

// 2. 同名のとき

#[tokio::test]
async fn same_name_skip_changes_nothing_and_makes_no_commit() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();
    let commits_before = fake.count_requests("POST", "/git/commits");
    let blobs_before = fake.count_requests("POST", "/git/blobs");

    let result = upload(
        &client,
        vec![item("docs", "a.txt", b"beta\n", OnExists::Skip)],
    )
    .await
    .expect("upload");

    assert_eq!(result.commit_sha, "", "commit は作られない");
    assert!(result.saved.is_empty());
    assert_eq!(result.skipped, vec!["docs/a.txt"]);
    assert!(result.renamed.is_empty());
    assert_eq!(fake.head(), before, "ref が動いていない");
    assert_eq!(fake.count_requests("POST", "/git/commits"), commits_before);
    assert_eq!(fake.count_requests("POST", "/git/blobs"), blobs_before);
    assert_eq!(fake.count_requests("PATCH", "/git/refs/heads/main"), 0);
    assert_eq!(read_file(&client, "docs", "a.txt").await, b"alpha\n");

    assert_request_rules(&fake);
}

#[tokio::test]
async fn same_name_replace_keeps_the_old_version_in_history() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();

    let result = upload(
        &client,
        vec![item("docs", "a.txt", b"beta\n", OnExists::Replace)],
    )
    .await
    .expect("upload");

    assert_eq!(result.saved, vec!["docs/a.txt"]);
    assert!(result.renamed.is_empty());
    assert_eq!(read_file(&client, "docs", "a.txt").await, b"beta\n");
    assert_eq!(read_file(&client, "", "README.md").await, b"# notes\n");

    // 古い版は履歴から取り出せる（新しい順）
    let history = client
        .file_history(&token(), OWNER, REPO, "docs/a.txt")
        .await
        .expect("file_history");
    let shas: Vec<&str> = history.iter().map(|v| v.sha.as_str()).collect();
    assert_eq!(shas, vec![result.commit_sha.as_str(), before.as_str()]);
    assert_eq!(history[0].message, "スマホから追加（1 件）");
    assert_eq!(history[1].message, INITIAL_MESSAGE);

    let (old_sha, old_size) = client
        .file_at(&token(), OWNER, REPO, "docs/a.txt", &before)
        .await
        .expect("file_at old");
    assert_eq!(old_size, 6);
    let old_bytes = client
        .fetch_blob(&token(), OWNER, REPO, &old_sha, Some(old_size))
        .await
        .expect("fetch old");
    assert_eq!(old_bytes, b"alpha\n");

    let (new_sha, _) = client
        .file_at(&token(), OWNER, REPO, "docs/a.txt", &result.commit_sha)
        .await
        .expect("file_at new");
    let new_bytes = client
        .fetch_blob(&token(), OWNER, REPO, &new_sha, None)
        .await
        .expect("fetch new");
    assert_eq!(new_bytes, b"beta\n");

    // 過去の版の一覧（list_dir の git_ref）でも古い内容のまま
    let old_listing = client
        .list_dir(&token(), OWNER, REPO, "docs", Some(&before))
        .await
        .expect("old listing");
    assert_eq!(old_listing.head_sha, before);
    assert_eq!(names(&old_listing), vec!["a.txt"]);

    assert_request_rules(&fake);
}

#[tokio::test]
async fn same_name_keep_both_numbers_the_name_each_time() {
    let fake = FakeGithub::start();
    let client = fake.client();

    let first = upload(
        &client,
        vec![item("docs", "a.txt", b"v2\n", OnExists::KeepBoth)],
    )
    .await
    .expect("first");
    assert_eq!(first.saved, vec!["docs/a (2).txt"]);
    assert_eq!(
        first.renamed,
        vec![Renamed {
            from: "docs/a.txt".to_string(),
            to: "docs/a (2).txt".to_string(),
        }]
    );

    let second = upload(
        &client,
        vec![item("docs", "a.txt", b"v3\n", OnExists::KeepBoth)],
    )
    .await
    .expect("second");
    assert_eq!(second.saved, vec!["docs/a (3).txt"]);
    assert_eq!(second.renamed.len(), 1);
    assert_eq!(second.renamed[0].to, "docs/a (3).txt");

    let listing = list(&client, "docs").await;
    assert_eq!(names(&listing), vec!["a (2).txt", "a (3).txt", "a.txt"]);
    assert_eq!(read_file(&client, "docs", "a.txt").await, b"alpha\n");
    assert_eq!(read_file(&client, "docs", "a (2).txt").await, b"v2\n");
    assert_eq!(read_file(&client, "docs", "a (3).txt").await, b"v3\n");

    assert_request_rules(&fake);
}

#[tokio::test]
async fn name_differing_only_in_case_uses_the_existing_spelling() {
    let fake = FakeGithub::start();
    let client = fake.client();

    // 計画は大文字小文字を区別せず「同名あり」とする
    let plan = client
        .plan_upload(
            &token(),
            OWNER,
            REPO,
            &[PlanItem {
                target_dir: String::new(),
                name: "README.MD".to_string(),
                size: 3,
            }],
        )
        .await
        .expect("plan");
    assert_eq!(plan.branch, "main");
    assert!(
        plan.items[0].exists,
        "README.MD は README.md と同名とみなす"
    );

    // 追加しない: 既存の表記（README.md）で報告し、何も変えない
    let before = fake.head();
    let skipped = upload(&client, vec![item("", "README.MD", b"x", OnExists::Skip)])
        .await
        .expect("skip");
    assert_eq!(skipped.skipped, vec!["README.md"]);
    assert_eq!(fake.head(), before);

    // 両方残す: 名前は大文字小文字の違いも含めて既存と重ならない形にする
    let kept = upload(
        &client,
        vec![item("", "README.MD", b"second\n", OnExists::KeepBoth)],
    )
    .await
    .expect("keep both");
    assert_eq!(kept.saved, vec!["README (2).MD"]);
    let root = list(&client, "").await;
    assert_eq!(
        names(&root),
        vec!["README (2).MD", "README.md", "docs", "資料"]
    );

    // 置き換える: 既存の README.md が新しい内容になり、README.MD という別のファイルは増えない
    let replaced = upload(
        &client,
        vec![item("", "readme.md", b"new\n", OnExists::Replace)],
    )
    .await
    .expect("replace");
    assert_eq!(replaced.saved, vec!["README.md"]);
    assert_eq!(read_file(&client, "", "README.md").await, b"new\n");

    assert_request_rules(&fake);
}

#[tokio::test]
async fn two_items_for_the_same_path_are_refused_before_any_write() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();

    let err = upload(
        &client,
        vec![
            item("docs", "new.txt", b"1", OnExists::Skip),
            item("docs", "NEW.txt", b"2", OnExists::Skip),
        ],
    )
    .await
    .expect_err("重複は拒否");
    assert!(matches!(err, RemoteError::DuplicateTarget(_)));
    assert_eq!(fake.head(), before);
    assert_eq!(fake.count_requests("POST", "/git/blobs"), 0);

    assert_request_rules(&fake);
}

// 3. 競合（別の端末が 1 回先に更新する）

#[tokio::test]
async fn one_remote_update_is_retried_and_both_files_survive() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();
    fake.queue_remote_update("他端末/from-other.txt", b"other\n");

    let result = upload(
        &client,
        vec![item("mine", "my.txt", b"mine\n", OnExists::Skip)],
    )
    .await
    .expect("retry が成功する");

    assert_eq!(result.saved, vec!["mine/my.txt"]);
    assert_eq!(fake.head(), result.commit_sha);

    // 作り直しは 2 回。PATCH は 422 の後に 200。blob は内容 1 件につき 1 回だけ作る
    assert_eq!(fake.count_requests("POST", "/git/commits"), 2);
    assert_eq!(fake.count_requests("PATCH", "/git/refs/heads/main"), 2);
    assert_eq!(fake.count_requests("POST", "/git/blobs"), 1);

    // 履歴は一本道: 新しい commit → 他端末の commit → 元の先頭
    let chain = fake.first_parent_chain(&result.commit_sha);
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[2], before);
    assert_eq!(chain[1], fake.commit(&chain[0]).expect("c").parents[0]);
    let remote = fake.commit(&chain[1]).expect("remote commit");
    assert_eq!(remote.message, REMOTE_MESSAGE);
    assert_eq!(remote.parents, vec![before.clone()]);
    assert_eq!(
        fake.commit(&chain[0]).expect("c").parents,
        vec![chain[1].clone()]
    );

    // 他端末のファイルも自分の追加も両方残る。既存のファイルも残る
    let files = fake.tree_files(&result.commit_sha);
    assert!(files.contains_key("他端末/from-other.txt"));
    assert!(files.contains_key("mine/my.txt"));
    assert!(files.contains_key("README.md"));
    assert!(files.contains_key("資料/議事録.txt"));
    assert_eq!(
        read_file(&client, "他端末", "from-other.txt").await,
        b"other\n"
    );
    assert_eq!(read_file(&client, "mine", "my.txt").await, b"mine\n");

    assert_request_rules(&fake);
}

#[tokio::test]
async fn remote_update_of_the_same_file_is_overwritten_by_replace_in_the_retry() {
    let fake = FakeGithub::start();
    let client = fake.client();
    fake.queue_remote_update("docs/a.txt", b"remote\n");

    let result = upload(
        &client,
        vec![item("docs", "a.txt", b"mine\n", OnExists::Replace)],
    )
    .await
    .expect("upload");
    assert_eq!(result.saved, vec!["docs/a.txt"]);
    assert_eq!(read_file(&client, "docs", "a.txt").await, b"mine\n");

    // 他端末の版は履歴に残る（作り直しの前に入った版）
    let history = client
        .file_history(&token(), OWNER, REPO, "docs/a.txt")
        .await
        .expect("history");
    assert_eq!(history.len(), 3);
    assert_eq!(history[1].message, REMOTE_MESSAGE);

    assert_request_rules(&fake);
}

#[tokio::test]
async fn keep_both_does_not_take_a_name_the_remote_just_created() {
    let fake = FakeGithub::start();
    let client = fake.client();
    fake.queue_remote_update("docs/a (2).txt", b"remote\n");

    let result = upload(
        &client,
        vec![item("docs", "a.txt", b"mine\n", OnExists::KeepBoth)],
    )
    .await
    .expect("upload");

    assert_eq!(result.saved, vec!["docs/a (3).txt"]);
    assert_eq!(
        result.renamed,
        vec![Renamed {
            from: "docs/a.txt".to_string(),
            to: "docs/a (3).txt".to_string(),
        }]
    );
    // 他端末が作った a (2).txt を上書きしない
    assert_eq!(read_file(&client, "docs", "a (2).txt").await, b"remote\n");
    assert_eq!(read_file(&client, "docs", "a (3).txt").await, b"mine\n");

    assert_request_rules(&fake);
}

// 4. 3 回続けて先に更新される

#[tokio::test]
async fn three_remote_updates_fail_with_not_fast_forward_and_leave_the_remote_as_it_was() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();
    fake.queue_remote_update("他端末/1.txt", b"1\n");
    fake.queue_remote_update("他端末/2.txt", b"2\n");
    fake.queue_remote_update("他端末/3.txt", b"3\n");

    let err = upload(
        &client,
        vec![item("mine", "x.txt", b"mine\n", OnExists::Skip)],
    )
    .await
    .expect_err("3 回とも重なれば失敗する");
    assert!(
        matches!(err, RemoteError::NotFastForward),
        "得た結果: {err:?}"
    );

    // 自分の追加はリポジトリに入らず、他端末の 3 回の更新はそのまま残る
    let head = fake.head();
    assert_ne!(head, before);
    let chain = fake.first_parent_chain(&head);
    assert_eq!(chain.len(), 4, "他端末の 3 件 + 元の 1 件だけ");
    assert_eq!(chain[3], before);
    for sha in &chain[..3] {
        assert_eq!(fake.commit(sha).expect("c").message, REMOTE_MESSAGE);
    }
    let files = fake.tree_files(&head);
    assert!(!files.contains_key("mine/x.txt"));
    for name in ["他端末/1.txt", "他端末/2.txt", "他端末/3.txt"] {
        assert!(files.contains_key(name), "{name} が残る");
    }
    assert_eq!(read_file(&client, "他端末", "2.txt").await, b"2\n");

    // 3 回の試行。PATCH は 3 回とも 422。blob は最初の 1 回だけ
    assert_eq!(fake.count_requests("PATCH", "/git/refs/heads/main"), 3);
    assert_eq!(fake.count_requests("POST", "/git/commits"), 3);
    assert_eq!(fake.count_requests("POST", "/git/blobs"), 1);

    assert_request_rules(&fake);
}

// 6. 途中の失敗（blob の作成の途中で 500）

#[tokio::test]
async fn failure_in_the_middle_of_blob_creation_leaves_the_ref_untouched_and_retry_works() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();
    // サーバー起動からの 2 回目の blob の作成だけを 500 にする
    fake.fail_blob_post(2);

    let items = vec![
        item("x", "1.txt", b"one", OnExists::Skip),
        item("x", "2.txt", b"two", OnExists::Skip),
        item("x", "3.txt", b"three", OnExists::Skip),
    ];
    let err = upload(&client, items).await.expect_err("500 で失敗する");
    assert!(
        !matches!(err, RemoteError::NotFastForward),
        "ネットワーク側の失敗として扱う: {err:?}"
    );
    assert!(
        !format!("{err}").contains(TOKEN_VALUE),
        "エラーの文言にトークンが出ない"
    );

    // ref は動いておらず、commit も ref の更新も作られていない
    assert_eq!(fake.head(), before);
    assert_eq!(fake.count_requests("POST", "/git/trees"), 0);
    assert_eq!(fake.count_requests("POST", "/git/commits"), 0);
    assert_eq!(fake.count_requests("PATCH", "/git/refs/heads/main"), 0);
    assert!(!fake.tree_files(&before).keys().any(|p| p.starts_with("x/")));

    // もう一度行うと成功する（失敗した分の blob は残っても、リポジトリの内容には出ない）
    let items = vec![
        item("x", "1.txt", b"one", OnExists::Skip),
        item("x", "2.txt", b"two", OnExists::Skip),
        item("x", "3.txt", b"three", OnExists::Skip),
    ];
    let result = upload(&client, items).await.expect("retry");
    assert_eq!(result.saved.len(), 3);
    assert_eq!(fake.head(), result.commit_sha);
    assert_eq!(read_file(&client, "x", "2.txt").await, b"two");
    assert_eq!(
        fake.commit(&result.commit_sha).expect("c").parents,
        vec![before]
    );

    assert_request_rules(&fake);
}

// 5. 規則の検査そのものの確かめ（疑似サーバーは禁止された呼び出しを受理しない）

#[tokio::test]
async fn fake_rejects_forbidden_calls_and_the_rule_check_reports_them() {
    let fake = FakeGithub::start();
    let before = fake.head();
    let http = reqwest::Client::new();
    let base = fake.base_url().to_string();
    let auth = format!("Bearer {TOKEN_VALUE}");

    // DELETE は 405
    let delete = http
        .delete(format!("{base}/repos/{OWNER}/{REPO}/git/refs/heads/main"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("delete");
    assert_eq!(delete.status().as_u16(), 405);

    // force: true の PATCH は 422 で、ref は動かない
    let forced = http
        .patch(format!("{base}/repos/{OWNER}/{REPO}/git/refs/heads/main"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"sha": before, "force": true}))
        .send()
        .await
        .expect("forced patch");
    assert_eq!(forced.status().as_u16(), 422);
    assert_eq!(fake.head(), before);

    // ref の作成（POST git/refs）は経路が無く 404
    let create_ref = http
        .post(format!("{base}/repos/{OWNER}/{REPO}/git/refs"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"ref": "refs/heads/x", "sha": before}))
        .send()
        .await
        .expect("create ref");
    assert_eq!(create_ref.status().as_u16(), 404);

    // 認証が無ければ 401
    let anonymous = http
        .get(format!("{base}/repos/{OWNER}/{REPO}"))
        .send()
        .await
        .expect("anonymous");
    assert_eq!(anonymous.status().as_u16(), 401);

    // 規則の検査は、これらを違反として報告する
    let v = violations(&fake.requests());
    assert!(v.iter().any(|m| m.contains("DELETE")), "{v:?}");
    assert!(v.iter().any(|m| m.contains("PATCH の本文")), "{v:?}");
    assert!(v.iter().any(|m| m.contains("POST が blobs")), "{v:?}");
    assert!(v.len() >= 3, "{v:?}");
}

// 5' 保存先の途中の階層が既存のファイルのとき（既存の検査は直接の保存先だけを見る）

#[tokio::test]
async fn upload_below_a_file_is_refused_with_a_path_error_and_changes_nothing() {
    let fake = FakeGithub::start();
    let client = fake.client();
    let before = fake.head();

    // docs/a.txt はファイル。その下の docs/a.txt/x に追加しようとする
    let err = upload(
        &client,
        vec![item("docs/a.txt/x", "inner.txt", b"x", OnExists::Skip)],
    )
    .await
    .expect_err("ファイルの下には追加できない");
    assert!(
        matches!(err, RemoteError::InvalidPath(_)),
        "保存先の検査で止まるべき（得た結果: {err:?}）"
    );
    assert_eq!(fake.head(), before, "ref は動かない");
    assert_eq!(fake.count_requests("POST", "/git/commits"), 0);

    assert_request_rules(&fake);
}
