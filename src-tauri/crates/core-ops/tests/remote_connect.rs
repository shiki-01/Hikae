// クラウドの保管場所の接続と、同期状態の表示用データの統合テスト。
// 実 git と、ローカルの bare リポジトリ（GitHub の代わり）だけを使う。実際の GitHub には通信しない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    project_folder_state, ConnectPreflight, FirstSave, FolderState, HistoryKind, Identity, Ops,
    OpsError, RemoteConnection, SizeLimits, UploadOutcome,
};
use std::path::PathBuf;

struct Setup {
    _tmp: tempfile::TempDir,
    work: PathBuf,
    bare: PathBuf,
    url: String,
    ops: Ops,
}

fn identity() -> Identity {
    Identity {
        name: "alice".to_string(),
        email: "1+alice@users.noreply.github.com".to_string(),
    }
}

/// 保存先の無いプロジェクトと、空の bare リポジトリ（未接続）を用意する
fn setup(name: &str) -> Setup {
    let tmp = tempfile::Builder::new()
        .prefix(name)
        .tempdir()
        .expect("tempdir");
    let work = tmp.path().join("work");
    let bare = tmp.path().join("remote.git");
    create_bare_repo(&bare).expect("bare");
    let ops = Ops::new(GitRunner::from_path_env());
    ops.init_project(&work, None, &identity()).expect("init");
    let url = bare.to_string_lossy().to_string();
    Setup {
        _tmp: tmp,
        work,
        bare,
        url,
        ops,
    }
}

fn remote_url(repo: &std::path::Path) -> Option<String> {
    let (code, out, _) = run_git(repo, &["remote", "get-url", "origin"]);
    (code == 0).then(|| out.trim().to_string())
}

#[test]
fn connecting_sets_origin_once_and_never_overwrites_a_different_place() {
    let s = setup("connect-origin");
    assert_eq!(remote_url(&s.work), None);

    assert_eq!(
        s.ops.connect_remote(&s.work, &s.url).expect("connect"),
        RemoteConnection::Added
    );
    assert_eq!(remote_url(&s.work).as_deref(), Some(s.url.as_str()));

    // 再試行しても同じ結果になる
    assert_eq!(
        s.ops.connect_remote(&s.work, &s.url).expect("again"),
        RemoteConnection::AlreadyConnected
    );

    // 別の場所へは付け替えない
    let err = s
        .ops
        .connect_remote(&s.work, "https://github.com/alice/other.git")
        .expect_err("different place");
    assert!(matches!(err, OpsError::InvalidInput(_)));
    assert_eq!(remote_url(&s.work).as_deref(), Some(s.url.as_str()));
}

#[test]
fn connecting_rejects_urls_that_could_leak_credentials_or_inject_options() {
    let s = setup("connect-reject");
    for bad in [
        "",
        "--upload-pack=evil",
        "https://x-access-token:secret@github.com/alice/a.git",
        "https://github.com/alice/a b.git",
    ] {
        let err = s.ops.connect_remote(&s.work, bad).expect_err(bad);
        assert!(matches!(err, OpsError::InvalidInput(_)), "{bad:?}");
    }
    assert_eq!(remote_url(&s.work), None);
}

#[test]
fn first_save_then_upload_puts_everything_on_the_remote_without_hikae_refs() {
    let s = setup("connect-first");
    write_test_file(&s.work, "メモ.txt", "最初の内容").expect("write");

    let outcome = s
        .ops
        .connect(&s.work, &s.url, Some("最初の保存"), SizeLimits::default())
        .expect("connect");
    assert_eq!(outcome.remote, RemoteConnection::Added);
    assert_eq!(outcome.first_save, FirstSave::Saved);
    // 初回の保存も復元点（自動保存）を作ってから行う
    let (_, refs, _) = run_git(&s.work, &["for-each-ref", "refs/hikae/"]);
    assert!(!refs.trim().is_empty(), "restore point should exist");

    // まだ上げていない: upstream は無いが、アップロード待ちの保存は 1 件
    assert!(!s.ops.sync_state(&s.work).expect("sync").has_upstream);
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 1);
    assert_eq!(s.ops.last_uploaded_at(&s.work).expect("time"), None);
    let before = s.ops.history(&s.work, 10).expect("history");
    assert_eq!(before.len(), 1);
    assert!(!before[0].cloud_synced);

    assert!(matches!(
        s.ops.upload(&s.work).expect("upload"),
        UploadOutcome::Pushed
    ));

    let head = get_head_commit(&s.work).expect("head");
    let (code, remote_head, _) = run_git(&s.bare, &["rev-parse", "main"]);
    assert_eq!(code, 0);
    assert_eq!(remote_head.trim(), head);
    assert!(has_no_hikae_refs_on_remote(&s.bare));
    assert!(s.ops.sync_state(&s.work).expect("sync").has_upstream);

    // 「ここまでクラウド」: 上げた保存はクラウド済み。最終アップロード日時は先端の保存の日時
    let after = s.ops.history(&s.work, 10).expect("history");
    assert!(after[0].cloud_synced);
    let (_, expected, _) = run_git(&s.bare, &["log", "-1", "--format=%cI", "main"]);
    assert_eq!(
        s.ops.last_uploaded_at(&s.work).expect("time"),
        Some(expected.trim().to_string())
    );
}

#[test]
fn history_marks_only_uploaded_manual_saves_as_cloud_synced() {
    let s = setup("connect-line");
    s.ops.connect_remote(&s.work, &s.url).expect("connect");

    write_test_file(&s.work, "a.txt", "1").expect("write");
    s.ops.save(&s.work, "一つ目").expect("save 1");
    write_test_file(&s.work, "a.txt", "2").expect("write");
    s.ops.save(&s.work, "二つ目").expect("save 2");
    s.ops.upload(&s.work).expect("upload");

    write_test_file(&s.work, "a.txt", "3").expect("write");
    s.ops.save(&s.work, "三つ目").expect("save 3");

    // 未保存の変更を持ったまま元に戻し、自動保存（復元点）を作る
    write_test_file(&s.work, "b.txt", "未保存").expect("write");
    let first = run_git(&s.work, &["rev-list", "--max-parents=0", "HEAD"]).1;
    s.ops.restore(&s.work, first.trim()).expect("restore");

    let history = s.ops.history(&s.work, 20).expect("history");
    let synced = |memo: &str| {
        history
            .iter()
            .find(|e| e.message == memo)
            .unwrap_or_else(|| panic!("no entry {memo}"))
            .cloud_synced
    };
    assert!(synced("一つ目"));
    assert!(synced("二つ目"));
    assert!(!synced("三つ目"));
    // 自動保存は、クラウドに上がることが無い
    let autos: Vec<_> = history
        .iter()
        .filter(|e| e.kind == HistoryKind::Auto)
        .collect();
    assert!(!autos.is_empty(), "an auto save should be listed");
    assert!(autos.iter().all(|e| !e.cloud_synced));
}

#[test]
fn nothing_is_cloud_synced_without_a_remote_or_before_the_first_upload() {
    let s = setup("connect-none");
    write_test_file(&s.work, "a.txt", "1").expect("write");
    s.ops.save(&s.work, "ローカルだけ").expect("save");

    // 保存先なし
    assert_eq!(s.ops.last_uploaded_at(&s.work).expect("time"), None);
    assert!(s
        .ops
        .history(&s.work, 10)
        .expect("history")
        .iter()
        .all(|e| !e.cloud_synced));

    // 保存先はあるが、まだ何も上げていない
    s.ops.connect_remote(&s.work, &s.url).expect("connect");
    assert_eq!(s.ops.last_uploaded_at(&s.work).expect("time"), None);
    assert!(s
        .ops
        .history(&s.work, 10)
        .expect("history")
        .iter()
        .all(|e| !e.cloud_synced));
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 1);
}

#[test]
fn upload_with_no_saves_yet_does_nothing_instead_of_failing() {
    let s = setup("connect-empty");
    let outcome = s
        .ops
        .connect(&s.work, &s.url, Some("最初の保存"), SizeLimits::default())
        .expect("connect");
    assert_eq!(outcome.first_save, FirstSave::NothingToSave);
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 0);
    assert!(matches!(
        s.ops.upload(&s.work).expect("upload"),
        UploadOutcome::NothingToUpload
    ));
    // クラウドには何も作られていない
    let (code, _, _) = run_git(&s.bare, &["rev-parse", "--verify", "--quiet", "main"]);
    assert_ne!(code, 0);
}

#[test]
fn first_save_with_a_large_file_changes_nothing_and_asks_for_a_decision() {
    let s = setup("connect-large");
    write_test_file(&s.work, "big.bin", &"x".repeat(200)).expect("write");
    let limits = SizeLimits {
        warn_bytes: 100,
        block_bytes: 1000,
    };
    let outcome = s
        .ops
        .connect(&s.work, &s.url, Some("最初の保存"), limits)
        .expect("connect");
    assert_eq!(outcome.remote, RemoteConnection::Added);
    let FirstSave::NeedsSizeDecision(found) = outcome.first_save else {
        panic!("expected a size decision");
    };
    assert_eq!(found.warned.len(), 1);
    // 保存も復元点も作っていない
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 0);
    let (_, refs, _) = run_git(&s.work, &["for-each-ref", "refs/hikae/"]);
    assert!(refs.trim().is_empty());
}

#[test]
fn a_pull_that_merges_keeps_the_remote_side_synced_and_local_work_unsynced() {
    let s = setup("connect-merge");
    s.ops.connect_remote(&s.work, &s.url).expect("connect");
    write_test_file(&s.work, "a.txt", "1").expect("write");
    s.ops.save(&s.work, "初回").expect("save");
    s.ops.upload(&s.work).expect("upload");

    // 別の PC が先に上げる
    let other = s.work.parent().expect("parent").join("other");
    let other_ops = Ops::new(GitRunner::from_path_env());
    other_ops
        .clone_project(
            &s.url,
            &other,
            &Identity {
                name: "bob".to_string(),
                email: "2+bob@users.noreply.github.com".to_string(),
            },
        )
        .expect("clone");
    write_test_file(&other, "b.txt", "other").expect("write");
    other_ops.save(&other, "別のPCの保存").expect("save");
    other_ops.upload(&other).expect("upload");

    // こちらも別の保存を作り、取り込む（双方に差があるのでマージになる）
    write_test_file(&s.work, "c.txt", "mine").expect("write");
    s.ops.save(&s.work, "こちらの保存").expect("save");
    s.ops.pull(&s.work).expect("pull");

    let history = s.ops.history(&s.work, 20).expect("history");
    let synced = |memo: &str| {
        history
            .iter()
            .find(|e| e.message == memo)
            .unwrap_or_else(|| panic!("no entry {memo}"))
            .cloud_synced
    };
    assert!(synced("初回"));
    assert!(synced("別のPCの保存"));
    assert!(!synced("こちらの保存"));
}

#[test]
fn folder_state_follows_the_real_folder() {
    let s = setup("connect-folder");
    assert_eq!(project_folder_state(&s.work), FolderState::Available);
    assert_eq!(
        project_folder_state(&s.work.join("none")),
        FolderState::Missing
    );
    std::fs::remove_dir_all(&s.work).expect("remove");
    assert_eq!(project_folder_state(&s.work), FolderState::Missing);
}

#[test]
fn preflight_is_ready_for_a_plain_project_and_changes_nothing() {
    let s = setup("preflight-ready");
    write_test_file(&s.work, "a.txt", "1").expect("write");
    let outcome = s
        .ops
        .preflight_connect(&s.work, &s.url, true, SizeLimits::default())
        .expect("preflight");
    assert_eq!(outcome, ConnectPreflight::Ready);
    // 検査は読み取りのみ: 保存先も復元点も保存も作られていない
    assert_eq!(remote_url(&s.work), None);
    let (_, refs, _) = run_git(&s.work, &["for-each-ref", "refs/hikae/"]);
    assert!(refs.trim().is_empty());
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 0);
}

#[test]
fn preflight_rejects_an_origin_that_points_elsewhere_and_accepts_the_same_place() {
    let s = setup("preflight-origin");
    s.ops
        .connect_remote(&s.work, "https://github.com/alice/other.git")
        .expect("connect");

    let elsewhere = s
        .ops
        .preflight_connect(
            &s.work,
            "https://github.com/alice/thesis.git",
            true,
            SizeLimits::default(),
        )
        .expect("preflight");
    assert_eq!(elsewhere, ConnectPreflight::OriginElsewhere);

    // 大文字小文字・末尾の .git の違いは同じ場所として扱う
    let same = s
        .ops
        .preflight_connect(
            &s.work,
            "https://github.com/Alice/Other",
            true,
            SizeLimits::default(),
        )
        .expect("preflight");
    assert_eq!(same, ConnectPreflight::AlreadyPointsHere);
    // 付け替えていない
    assert_eq!(
        remote_url(&s.work).as_deref(),
        Some("https://github.com/alice/other.git")
    );
}

#[test]
fn preflight_detects_a_config_that_cannot_be_written() {
    let s = setup("preflight-config");
    let config = s.work.join(".git").join("config");
    let mut perms = std::fs::metadata(&config).expect("meta").permissions();
    let original = perms.clone();
    perms.set_readonly(true);
    std::fs::set_permissions(&config, perms).expect("readonly");

    // 管理者権限などで読み取り専用でも書けてしまう環境では、検査の対象外にする
    let writable_anyway = std::fs::OpenOptions::new()
        .append(true)
        .open(&config)
        .is_ok();
    let outcome = s
        .ops
        .preflight_connect(&s.work, &s.url, false, SizeLimits::default())
        .expect("preflight");
    std::fs::set_permissions(&config, original).expect("restore");

    if !writable_anyway {
        assert_eq!(outcome, ConnectPreflight::ConfigNotWritable);
    }
    assert_eq!(remote_url(&s.work), None);
}

#[test]
fn preflight_reports_a_missing_or_non_repository_folder() {
    let s = setup("preflight-folder");
    let missing = s.work.join("none");
    assert_eq!(
        s.ops
            .preflight_connect(&missing, &s.url, true, SizeLimits::default())
            .expect("preflight"),
        ConnectPreflight::FolderUnavailable(FolderState::Missing)
    );
    let plain = s.work.parent().expect("parent").join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    assert_eq!(
        s.ops
            .preflight_connect(&plain, &s.url, true, SizeLimits::default())
            .expect("preflight"),
        ConnectPreflight::FolderUnavailable(FolderState::NotARepository)
    );
}

#[test]
fn preflight_asks_for_a_size_decision_before_anything_is_created() {
    let s = setup("preflight-large");
    write_test_file(&s.work, "big.bin", &"x".repeat(2000)).expect("write");
    write_test_file(&s.work, "mid.bin", &"x".repeat(200)).expect("write");
    let limits = SizeLimits {
        warn_bytes: 100,
        block_bytes: 1000,
    };

    let outcome = s
        .ops
        .preflight_connect(&s.work, &s.url, true, limits)
        .expect("preflight");
    let ConnectPreflight::NeedsSizeDecision(found) = outcome else {
        panic!("expected a size decision, got {outcome:?}");
    };
    assert_eq!(found.blocked.len(), 1);
    assert_eq!(found.blocked[0].path, "big.bin");
    assert_eq!(found.warned.len(), 1);
    // 何も変更していない（保存先も復元点も保存も無い）
    assert_eq!(remote_url(&s.work), None);
    assert_eq!(s.ops.commit_count(&s.work).expect("count"), 0);
    let (_, refs, _) = run_git(&s.work, &["for-each-ref", "refs/hikae/"]);
    assert!(refs.trim().is_empty());

    // 初回の保存を頼まないときは、サイズ検査をしない
    assert_eq!(
        s.ops
            .preflight_connect(&s.work, &s.url, false, limits)
            .expect("preflight"),
        ConnectPreflight::Ready
    );
}

#[test]
fn preflight_skips_the_size_check_once_a_save_exists() {
    let s = setup("preflight-saved");
    write_test_file(&s.work, "a.txt", "1").expect("write");
    s.ops.save(&s.work, "最初").expect("save");
    write_test_file(&s.work, "big.bin", &"x".repeat(2000)).expect("write");
    let limits = SizeLimits {
        warn_bytes: 100,
        block_bytes: 1000,
    };
    // すでに保存があるプロジェクトの初回の保存は作らないため、検査の対象外
    assert_eq!(
        s.ops
            .preflight_connect(&s.work, &s.url, true, limits)
            .expect("preflight"),
        ConnectPreflight::Ready
    );
}

#[test]
fn preflight_rejects_urls_that_could_leak_credentials() {
    let s = setup("preflight-url");
    let err = s
        .ops
        .preflight_connect(
            &s.work,
            "https://x-access-token:secret@github.com/alice/a.git",
            true,
            SizeLimits::default(),
        )
        .expect_err("credentials");
    assert!(matches!(err, OpsError::InvalidInput(_)));
}
