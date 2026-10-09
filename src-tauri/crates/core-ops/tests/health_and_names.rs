// E10（リポジトリの破損）・E17（git の確認）・E18（取り込み前のファイル名の検査）・E09（容量）の統合テスト。
// 実 git と、ローカルの bare リポジトリ（GitHub の代わり）だけを使う。実際の GitHub には通信しない。
// 実際に Windows で作れない名前のファイルは作らない（git のプラミングで、取り込む側のツリーにだけ載せる）。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    probe_git, BrokenReason, GitProbe, Identity, Ops, OpsError, PullOutcome, RepoHealth,
    UnsupportedReason,
};
use std::path::{Path, PathBuf};

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn new_ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

/// 保存が 1 件あるプロジェクト
fn project_with_commit(tmp: &Path, name: &str) -> PathBuf {
    let dir = tmp.join(name);
    let ops = new_ops();
    ops.init_project(&dir, None, &identity(name)).expect("init");
    write_test_file(&dir, "a.txt", "hello").expect("write");
    ops.save(&dir, "first").expect("save");
    dir
}

// ---- E17: git の確認 ----

#[test]
fn probe_accepts_a_working_git() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        probe_git(&GitRunner::from_path_env(), tmp.path()),
        GitProbe::Available
    );
}

#[test]
fn probe_detects_a_missing_git_executable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("no-such-dir").join("git-not-here");
    assert_eq!(
        probe_git(&GitRunner::new(missing), tmp.path()),
        GitProbe::NotFound
    );
}

#[test]
fn probe_detects_a_file_that_is_not_git() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // 実行できないファイルを git として指定した場合は、見つからないのではなく壊れているとみなす
    let fake = tmp.path().join("git");
    std::fs::write(&fake, b"this is not an executable").expect("write");
    let probe = probe_git(&GitRunner::new(fake), tmp.path());
    assert!(matches!(probe, GitProbe::Broken(_)), "{probe:?}");
}

// ---- E10: リポジトリの破損 ----

#[test]
fn a_healthy_repository_passes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = project_with_commit(tmp.path(), "ok");
    let ops = new_ops();
    assert_eq!(
        ops.check_repo_health(&dir, true).expect("check"),
        RepoHealth::Healthy
    );
    assert_eq!(
        ops.check_repo_health(&dir, false).expect("check"),
        RepoHealth::Healthy
    );
}

#[test]
fn a_repository_without_saves_is_healthy_unless_saves_are_expected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("empty");
    let ops = new_ops();
    ops.init_project(&dir, None, &identity("empty"))
        .expect("init");
    assert_eq!(
        ops.check_repo_health(&dir, false).expect("check"),
        RepoHealth::Healthy
    );
    assert_eq!(
        ops.check_repo_health(&dir, true).expect("check"),
        RepoHealth::Broken(BrokenReason::HeadMissing)
    );
}

#[test]
fn a_damaged_head_file_is_detected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = project_with_commit(tmp.path(), "badhead");
    std::fs::write(dir.join(".git").join("HEAD"), b"\0\0garbage").expect("damage");
    assert_eq!(
        new_ops().check_repo_health(&dir, true).expect("check"),
        RepoHealth::Broken(BrokenReason::NotARepository)
    );
}

/// 読み取り専用のオブジェクトファイルを消す（Windows では読み取り専用だと消せない）
#[allow(clippy::permissions_set_readonly_false)]
fn delete_object(repo: &Path, oid: &str) {
    let path = repo
        .join(".git")
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..]);
    let mut perms = std::fs::metadata(&path)
        .expect("object exists")
        .permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).expect("chmod");
    std::fs::remove_file(&path).expect("delete object");
}

#[test]
fn a_missing_head_commit_object_is_detected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = project_with_commit(tmp.path(), "nocommit");
    let head = get_head_commit(&dir).expect("head");
    delete_object(&dir, &head);
    assert_eq!(
        new_ops().check_repo_health(&dir, true).expect("check"),
        RepoHealth::Broken(BrokenReason::HeadObjectMissing)
    );
}

#[test]
fn a_missing_root_tree_object_is_detected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = project_with_commit(tmp.path(), "notree");
    let (code, tree, _) = run_git(&dir, &["rev-parse", "HEAD^{tree}"]);
    assert_eq!(code, 0);
    delete_object(&dir, tree.trim());
    assert_eq!(
        new_ops().check_repo_health(&dir, true).expect("check"),
        RepoHealth::Broken(BrokenReason::HeadObjectMissing)
    );
}

#[test]
fn a_missing_git_dir_is_reported_as_not_a_repository() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    // 親にリポジトリが無い場所では、リポジトリとして読めない
    let health = new_ops().check_repo_health(&plain, false).expect("check");
    assert_eq!(health, RepoHealth::Broken(BrokenReason::NotARepository));
}

// ---- E09: 容量 ----

#[test]
fn repo_size_is_read_from_count_objects() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = project_with_commit(tmp.path(), "size");
    let size = new_ops().repo_size(&dir).expect("size");
    assert!(!size.exceeds_warning());
}

// ---- E18: 取り込み前のファイル名の検査 ----

struct Pair {
    _tmp: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

/// A と B が同じ bare リポジトリを共有する状態にする（最初の保存は A が上げて、B が取得する）
fn pair(name: &str) -> Pair {
    let tmp = tempfile::Builder::new()
        .prefix(name)
        .tempdir()
        .expect("tempdir");
    let bare = tmp.path().join("remote.git");
    create_bare_repo(&bare).expect("bare");
    let url = bare.to_string_lossy().to_string();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");

    let ops = new_ops();
    ops.init_project(&a, Some(&url), &identity("pc-a"))
        .expect("init a");
    write_test_file(&a, "file.txt", "initial").expect("write");
    ops.save(&a, "initial").expect("save");
    ops.upload(&a).expect("upload");
    ops.clone_project(&url, &b, &identity("pc-b"))
        .expect("clone b");
    Pair { _tmp: tmp, a, b }
}

/// B に、Windows では作れない名前を含む保存を、作業フォルダを経由せずに作って上げる
fn push_unsupported_names_from_b(p: &Pair, paths: &[&str]) {
    for path in paths {
        let (code, oid, err) = run_git_stdin(&p.b, &["hash-object", "-w", "--stdin"], "x");
        assert_eq!(code, 0, "{err}");
        let cacheinfo = format!("100644,{},{path}", oid.trim());
        let (code, _, err) = run_git(
            &p.b,
            &[
                "-c",
                "core.protectNTFS=false",
                "update-index",
                "--add",
                "--cacheinfo",
                &cacheinfo,
            ],
        );
        assert_eq!(code, 0, "{path}: {err}");
    }
    let (code, _, err) = run_git(
        &p.b,
        &[
            "-c",
            "user.name=pc-b",
            "-c",
            "user.email=pc-b@example.com",
            "commit",
            "-m",
            "odd names",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run_git(&p.b, &["push", "origin", "HEAD"]);
    assert_eq!(code, 0, "{err}");
}

fn refs_under_hikae(repo: &Path) -> String {
    let (_, out, _) = run_git(repo, &["for-each-ref", "refs/hikae/"]);
    out
}

#[test]
fn pull_refuses_names_windows_cannot_create_and_changes_nothing() {
    let p = pair("e18-refuse");
    push_unsupported_names_from_b(&p, &["Aux.txt", "docs/memo.", "ok.txt"]);

    let head_before = get_head_commit(&p.a).expect("head");
    let refs_before = refs_under_hikae(&p.a);
    // 未保存の変更があっても、検査は復元点や自動保存より前に行う
    write_test_file(&p.a, "draft.txt", "unsaved").expect("write");

    let ops = new_ops().with_windows_name_check(true);
    let err = ops.pull(&p.a).expect_err("must be refused");
    let OpsError::UnsupportedFileNames(names) = err else {
        panic!("expected UnsupportedFileNames, got {err:?}");
    };
    let listed: Vec<(&str, UnsupportedReason)> =
        names.iter().map(|n| (n.path.as_str(), n.reason)).collect();
    assert_eq!(
        listed,
        vec![
            ("Aux.txt", UnsupportedReason::ReservedName),
            ("docs/memo.", UnsupportedReason::TrailingDotOrSpace),
        ]
    );

    // 何も取り込まず、保存も復元点も作っていない
    assert_eq!(get_head_commit(&p.a).expect("head"), head_before);
    assert_eq!(refs_under_hikae(&p.a), refs_before);
    assert!(p.a.join("draft.txt").exists());
    assert!(!p.a.join("ok.txt").exists());
    let (_, status, _) = run_git(&p.a, &["status", "--porcelain"]);
    assert!(status.contains("draft.txt"), "{status}");
}

#[test]
fn pull_without_the_check_is_not_affected() {
    let p = pair("e18-off");
    // 検査を無効にした場合は、普通の名前の取り込みがそのまま成功する
    write_test_file(&p.b, "new.txt", "from b").expect("write");
    let ops_b = new_ops().with_windows_name_check(false);
    ops_b.save(&p.b, "add new").expect("save");
    ops_b.upload(&p.b).expect("upload");

    let ops = new_ops().with_windows_name_check(false);
    assert!(matches!(
        ops.pull(&p.a).expect("pull"),
        PullOutcome::FastForwarded
    ));
    assert!(p.a.join("new.txt").exists());
}

#[test]
fn pull_with_the_check_accepts_ordinary_names() {
    let p = pair("e18-ok");
    write_test_file(&p.b, "docs/第3章.docx", "from b").expect("write");
    let ops_b = new_ops();
    ops_b.save(&p.b, "add chapter").expect("save");
    ops_b.upload(&p.b).expect("upload");

    let ops = new_ops().with_windows_name_check(true);
    assert!(matches!(
        ops.pull(&p.a).expect("pull"),
        PullOutcome::FastForwarded
    ));
    assert!(p.a.join("docs").join("第3章.docx").exists());
}

#[test]
fn pull_with_nothing_incoming_skips_the_check() {
    let p = pair("e18-uptodate");
    let ops = new_ops().with_windows_name_check(true);
    assert!(matches!(
        ops.pull(&p.a).expect("pull"),
        PullOutcome::UpToDate
    ));
}

#[test]
fn upload_that_needs_a_pull_also_refuses_unsupported_names() {
    let p = pair("e18-upload");
    push_unsupported_names_from_b(&p, &["Nul"]);
    write_test_file(&p.a, "mine.txt", "local").expect("write");
    let ops = new_ops().with_windows_name_check(true);
    ops.save(&p.a, "mine").expect("save");
    let err = ops.upload(&p.a).expect_err("must be refused");
    assert!(matches!(err, OpsError::UnsupportedFileNames(_)), "{err:?}");
    // 自分の保存は残っている
    assert!(p.a.join("mine.txt").exists());
}
