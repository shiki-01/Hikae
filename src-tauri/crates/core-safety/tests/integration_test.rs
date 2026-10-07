mod common;

use core_git::GitRunner;
use core_safety::{create_backup_ref, create_restore_point, create_snapshot, list_snapshots};
use std::fs;
use tempfile::TempDir;
use time::macros::datetime;

/// テスト用のリポジトリをセットアップ
fn setup_test_repo() -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // git init
    common::git_command(&repo_path, &["init"]);

    // core.autocrlf=false を設定
    common::git_command(&repo_path, &["config", "--local", "core.autocrlf", "false"]);
    common::git_command(
        &repo_path,
        &["config", "--local", "core.precomposeUnicode", "true"],
    );

    // user 情報を設定（global を避けるためローカルに）
    common::git_command(
        &repo_path,
        &["config", "--local", "user.email", "test@example.com"],
    );
    common::git_command(&repo_path, &["config", "--local", "user.name", "Test User"]);

    // 初期コミット
    fs::write(repo_path.join("a.txt"), "content a\n").expect("failed to write a.txt");
    fs::write(repo_path.join("b.txt"), "content b\n").expect("failed to write b.txt");

    let sub_dir = repo_path.join("sub");
    fs::create_dir(&sub_dir).expect("failed to create sub dir");
    fs::write(sub_dir.join("c.txt"), "content c\n").expect("failed to write c.txt");

    common::git_command(&repo_path, &["add", "a.txt", "b.txt", "sub/c.txt"]);
    common::git_command(&repo_path, &["commit", "-m", "initial"]);

    (temp_dir, repo_path)
}

/// テスト後のクリーンアップ（手動で明示的）
fn cleanup(temp_dir: TempDir) {
    drop(temp_dir);
}

#[test]
fn test_create_snapshot_basic() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // a.txt を変更（未ステージ）
    fs::write(repo_path.join("a.txt"), "modified a\n").expect("failed to write a.txt");

    // b.txt を変更してステージング
    fs::write(repo_path.join("b.txt"), "modified b\n").expect("failed to write b.txt");
    common::git_command(&repo_path, &["add", "b.txt"]);

    // sub/c.txt を削除
    fs::remove_file(repo_path.join("sub/c.txt")).expect("failed to delete c.txt");

    // 新しいファイルを追加（日本語・空白を含む名前）
    fs::write(repo_path.join("新規 ファイル.txt"), "new file content\n")
        .expect("failed to write new file");

    // .gitignore で ignored.log を追加
    fs::write(repo_path.join(".gitignore"), "*.log\n").expect("failed to write .gitignore");
    fs::write(repo_path.join("ignored.log"), "ignored\n").expect("failed to write ignored.log");

    // スナップショット作成直前の状態を記録
    let working_tree_before =
        common::hash_working_tree(&repo_path).expect("failed to hash working tree before");
    let index_before = common::read_index_file(&repo_path).expect("failed to read index before");

    // スナップショットを作成
    let now = datetime!(2026-10-07 14:30:45 UTC);
    let snapshot = create_snapshot(&runner, &repo_path, "main", now)
        .expect("failed to create snapshot")
        .expect("snapshot should not be None");

    // スナップショット作成直後の状態を記録
    let working_tree_after =
        common::hash_working_tree(&repo_path).expect("failed to hash working tree after");

    // 作業フォルダが変わっていないことを確認
    assert_eq!(
        working_tree_before, working_tree_after,
        "working tree should not change during snapshot"
    );

    // インデックスが変わっていないことを確認
    let index_after = common::read_index_file(&repo_path).expect("failed to read index after");
    assert_eq!(
        index_before, index_after,
        "index should not change after snapshot"
    );

    // .git/index.lock が存在しないことを確認
    assert!(
        common::check_no_index_lock(&repo_path),
        ".git/index.lock should not exist"
    );

    // 一時インデックスが残っていないことを確認
    assert!(
        common::check_no_temp_index(&repo_path),
        "temp index files should be cleaned up"
    );

    // ref が正しい形式であることを確認
    assert!(snapshot.ref_name.starts_with("refs/hikae/snapshots/main/"));

    // commit と tree が 40 または 64 桁の 16 進数であることを確認
    assert!(snapshot.commit.len() == 40 || snapshot.commit.len() == 64);
    assert!(snapshot.tree.len() == 40 || snapshot.tree.len() == 64);

    cleanup(_temp);
}

#[test]
fn test_snapshot_deduplication_same_tree() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // ファイルを変更
    fs::write(repo_path.join("a.txt"), "modified\n").expect("failed to write");

    let now1 = datetime!(2026-10-07 14:30:45 UTC);

    // 最初のスナップショット
    let _snap1 = create_snapshot(&runner, &repo_path, "main", now1)
        .expect("failed to create snapshot 1")
        .expect("first snapshot should exist");

    // 何も変更しないで再度スナップショット作成
    let now2 = datetime!(2026-10-07 14:31:00 UTC);
    let snap2 =
        create_snapshot(&runner, &repo_path, "main", now2).expect("failed to create snapshot 2");

    // 2 つ目は None であるべき（ツリーが同一）
    assert!(
        snap2.is_none(),
        "snapshot should be deduplicated when tree is same"
    );

    cleanup(_temp);
}

#[test]
fn test_snapshot_creates_when_changed() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // 最初の変更
    fs::write(repo_path.join("a.txt"), "content v1\n").expect("failed to write");
    let now1 = datetime!(2026-10-07 14:30:45 UTC);
    let snap1 = create_snapshot(&runner, &repo_path, "main", now1)
        .expect("failed to create snapshot 1")
        .expect("first snapshot should exist");

    // ファイルを別の内容に変更
    fs::write(repo_path.join("a.txt"), "content v2\n").expect("failed to write");

    let now2 = datetime!(2026-10-07 14:31:00 UTC);
    let snap2 = create_snapshot(&runner, &repo_path, "main", now2)
        .expect("failed to create snapshot 2")
        .expect("second snapshot should exist when changed");

    // 2 つのスナップショットが異なることを確認
    assert_ne!(snap1.tree, snap2.tree, "trees should be different");

    cleanup(_temp);
}

#[test]
fn test_snapshot_not_created_when_matching_head() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // 作業フォルダが HEAD と完全一致の状態でスナップショット
    let now = datetime!(2026-10-07 14:30:45 UTC);
    let snap =
        create_snapshot(&runner, &repo_path, "main", now).expect("failed to create snapshot");

    // 作業フォルダが HEAD と完全一致しているので None
    assert!(
        snap.is_none(),
        "snapshot should not be created when working tree matches HEAD"
    );

    cleanup(_temp);
}

#[test]
fn test_snapshot_with_zero_commits() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // git init のみ（コミットなし）
    common::git_command(&repo_path, &["init"]);
    common::git_command(&repo_path, &["config", "--local", "core.autocrlf", "false"]);

    // ファイルを追加
    fs::write(repo_path.join("file.txt"), "content\n").expect("failed to write file.txt");

    let runner = GitRunner::from_path_env();
    let now = datetime!(2026-10-07 14:30:45 UTC);

    let snap = create_snapshot(&runner, &repo_path, "main", now)
        .expect("failed to create snapshot with zero commits")
        .expect("snapshot should exist even with zero commits");

    assert!(snap.ref_name.starts_with("refs/hikae/snapshots/main/"));

    cleanup(temp_dir);
}

#[test]
fn test_backup_ref_creation() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    let now = datetime!(2026-10-07 14:30:45 UTC);
    let backup = create_backup_ref(&runner, &repo_path, "merge", now)
        .expect("failed to create backup ref")
        .expect("backup ref should exist");

    assert!(backup.starts_with("refs/hikae/backup/merge/"));

    cleanup(_temp);
}

#[test]
fn test_backup_ref_with_zero_commits() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    common::git_command(&repo_path, &["init"]);
    common::git_command(&repo_path, &["config", "--local", "core.autocrlf", "false"]);

    let runner = GitRunner::from_path_env();
    let now = datetime!(2026-10-07 14:30:45 UTC);

    let backup =
        create_backup_ref(&runner, &repo_path, "merge", now).expect("failed to create backup ref");

    // コミット 0 件なら None
    assert!(backup.is_none());

    cleanup(temp_dir);
}

#[test]
fn test_list_snapshots() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // 複数のスナップショットを作成
    fs::write(repo_path.join("a.txt"), "v1\n").expect("failed to write");
    let now1 = datetime!(2026-10-07 14:30:00 UTC);
    let s1 =
        create_snapshot(&runner, &repo_path, "main", now1).expect("failed to create snapshot 1");
    assert!(s1.is_some(), "first snapshot should be created");

    fs::write(repo_path.join("a.txt"), "v2\n").expect("failed to write");
    let now2 = datetime!(2026-10-07 14:31:00 UTC);
    let s2 =
        create_snapshot(&runner, &repo_path, "main", now2).expect("failed to create snapshot 2");
    assert!(s2.is_some(), "second snapshot should be created");

    fs::write(repo_path.join("a.txt"), "v3\n").expect("failed to write");
    let now3 = datetime!(2026-10-07 14:32:00 UTC);
    let s3 =
        create_snapshot(&runner, &repo_path, "main", now3).expect("failed to create snapshot 3");
    assert!(s3.is_some(), "third snapshot should be created");

    // 列挙（新しい順）
    let snapshots = list_snapshots(&runner, &repo_path, "main").expect("failed to list snapshots");

    assert_eq!(snapshots.len(), 3, "should have 3 snapshots");

    // 新しい順か確認（ref name でソート）
    for i in 0..snapshots.len() - 1 {
        assert!(
            snapshots[i].ref_name > snapshots[i + 1].ref_name,
            "snapshots should be in descending order"
        );
    }

    cleanup(_temp);
}

#[test]
fn test_invalid_branch_name() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();
    let now = datetime!(2026-10-07 14:30:45 UTC);

    // 空の branch 名
    assert!(
        create_snapshot(&runner, &repo_path, "", now).is_err(),
        "empty branch name should be rejected"
    );

    // ".." を含む
    assert!(
        create_snapshot(&runner, &repo_path, "main..test", now).is_err(),
        "'..' in branch name should be rejected"
    );

    // 先頭に /
    assert!(
        create_snapshot(&runner, &repo_path, "/main", now).is_err(),
        "branch starting with '/' should be rejected"
    );

    cleanup(_temp);
}

#[test]
fn test_invalid_operation_name() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();
    let now = datetime!(2026-10-07 14:30:45 UTC);

    // 大文字を含む
    assert!(
        create_backup_ref(&runner, &repo_path, "MERGE", now).is_err(),
        "uppercase in operation name should be rejected"
    );

    // アンダースコア
    assert!(
        create_backup_ref(&runner, &repo_path, "op_test", now).is_err(),
        "underscore in operation name should be rejected"
    );

    cleanup(_temp);
}

#[test]
fn test_restore_point() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // ファイルを変更
    fs::write(repo_path.join("a.txt"), "modified\n").expect("failed to write");

    let now = datetime!(2026-10-07 14:30:45 UTC);
    let restore_point = create_restore_point(&runner, &repo_path, "main", "merge", now)
        .expect("failed to create restore point");

    // snapshot と backup ref の両方が存在するはず
    assert!(restore_point.snapshot.is_some(), "snapshot should exist");
    assert!(
        restore_point.backup_ref.is_some(),
        "backup ref should exist"
    );

    cleanup(_temp);
}

#[test]
fn test_snapshot_not_in_public_history() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();
    let bare_path = TempDir::new().expect("failed to create bare dir");

    // ローカルリポを初期化
    common::git_command(&repo_path, &["init"]);
    common::git_command(&repo_path, &["config", "--local", "core.autocrlf", "false"]);
    common::git_command(
        &repo_path,
        &["config", "--local", "user.email", "test@example.com"],
    );
    common::git_command(&repo_path, &["config", "--local", "user.name", "Test"]);

    // 初期コミット
    fs::write(repo_path.join("file.txt"), "content\n").expect("failed to write");
    common::git_command(&repo_path, &["add", "file.txt"]);
    common::git_command(&repo_path, &["commit", "-m", "init"]);

    // bare リポを作る
    common::git_command(bare_path.path(), &["init", "--bare"]);

    // remote を追加
    common::git_command(
        &repo_path,
        &[
            "remote",
            "add",
            "origin",
            bare_path.path().to_str().unwrap(),
        ],
    );

    // ローカルで snapshot を作成
    let runner = GitRunner::from_path_env();
    fs::write(repo_path.join("file.txt"), "changed\n").expect("failed to write");
    let now = datetime!(2026-10-07 14:30:45 UTC);
    create_snapshot(&runner, &repo_path, "main", now).expect("failed to create snapshot");

    // push
    common::git_command(&repo_path, &["push", "-u", "origin", "HEAD"]);

    // bare 側に refs/hikae/ が存在しないことを確認
    let bare_refs_output = std::process::Command::new("git")
        .args(["for-each-ref", "refs/hikae/"])
        .current_dir(bare_path.path())
        .output()
        .expect("failed to run for-each-ref");

    let refs_output = String::from_utf8_lossy(&bare_refs_output.stdout);
    assert!(
        refs_output.trim().is_empty(),
        "refs/hikae/ should not exist in bare repo"
    );

    cleanup(temp_dir);
    cleanup(bare_path);
}

#[test]
fn test_ref_collision_handling() {
    let (_temp, repo_path) = setup_test_repo();
    let runner = GitRunner::from_path_env();

    // 同じ時刻で 2 回スナップショットを作成（ref が衝突）
    fs::write(repo_path.join("a.txt"), "v1\n").expect("failed to write");
    let now = datetime!(2026-10-07 14:30:45 UTC);

    let snap1 = create_snapshot(&runner, &repo_path, "main", now)
        .expect("failed to create snapshot 1")
        .expect("snapshot 1 should exist");

    fs::write(repo_path.join("a.txt"), "v2\n").expect("failed to write");

    let snap2 = create_snapshot(&runner, &repo_path, "main", now)
        .expect("failed to create snapshot 2")
        .expect("snapshot 2 should exist");

    // ref が異なることを確認（衝突回避）
    assert_ne!(
        snap1.ref_name, snap2.ref_name,
        "refs should be unique even with same timestamp"
    );

    // 両方とも refs/hikae/snapshots/main/ の下にあることを確認
    assert!(snap1.ref_name.starts_with("refs/hikae/snapshots/main/"));
    assert!(snap2.ref_name.starts_with("refs/hikae/snapshots/main/"));

    cleanup(_temp);
}
