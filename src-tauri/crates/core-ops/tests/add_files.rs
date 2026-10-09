// ファイル・フォルダの追加（Ops::add_files。設計書 4.7）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。同名の扱い（確認・両方残す・置き換え）、
// 置き換えの安全策（復元点に内容が入っていること、入らないものは置き換えない）、フォルダの再帰と上限、
// リンク・隠しファイルの扱いを確かめる。不変条件 3（復元点）・4（インデックスを変えない）が目的。
// 時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    AddConflictAction, AddConflictDecision, AddConflictPolicy, AddFilesOutcome, AddFilesRequest,
    AddRefusal, AddRejectReason, Identity, Ops, OpsError, SizeLimits, SkipReason,
};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LIMITS: SizeLimits = SizeLimits {
    warn_bytes: 1024,
    block_bytes: 10 * 1024,
};

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(
        &repo,
        None,
        &Identity {
            name: "tester".to_string(),
            email: "tester@users.noreply.github.com".to_string(),
        },
    )?;
    Ok(repo)
}

/// コピー元の置き場（プロジェクトの外）
fn source_dir(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = tmp.join("outside");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

fn request(sources: Vec<PathBuf>, dest: &str, policy: AddConflictPolicy) -> AddFilesRequest {
    AddFilesRequest {
        sources,
        dest_subdir: dest.to_string(),
        on_conflict: policy,
        decisions: Vec::new(),
        limits: LIMITS,
    }
}

fn add(repo: &Path, req: &AddFilesRequest) -> Result<AddFilesOutcome, OpsError> {
    ops().add_files(repo, req)
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| format!("<{e}>"))
}

fn backup_count(repo: &Path) -> usize {
    git_out(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hikae/backup/add-files/",
        ],
    )
    .lines()
    .count()
}

#[test]
fn ask_writes_nothing_and_lists_the_files_that_need_a_decision() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "a.txt", "プロジェクトの a")?;
    ops().save(&repo, "first")?;
    std::fs::write(src.join("a.txt"), "外の a")?;
    std::fs::write(src.join("b.txt"), "外の b")?;
    let index_before = git_out(&repo, &["ls-files", "-s"]);

    let out = add(
        &repo,
        &request(
            vec![src.join("a.txt"), src.join("b.txt")],
            "",
            AddConflictPolicy::Ask,
        ),
    )?;

    // 同名があるときは、同名でないファイルも含めて何も書かない（復元点も作らない）
    assert!(out.added.is_empty());
    assert_eq!(out.needs_decision.len(), 1);
    assert_eq!(out.needs_decision[0].path, "a.txt");
    assert!(out.needs_decision[0].can_replace);
    assert_eq!(read(&repo, "a.txt"), "プロジェクトの a");
    assert!(!repo.join("b.txt").exists());
    assert_eq!(backup_count(&repo), 0);
    assert_eq!(git_out(&repo, &["ls-files", "-s"]), index_before);

    // 同名が無ければ、確認なしでそのまま追加する
    let out = add(
        &repo,
        &request(vec![src.join("b.txt")], "", AddConflictPolicy::Ask),
    )?;
    assert_eq!(out.added.len(), 1);
    assert!(out.needs_decision.is_empty());
    assert_eq!(read(&repo, "b.txt"), "外の b");
    Ok(())
}

#[test]
fn keep_both_numbers_the_new_file_and_never_overwrites() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "a.txt", "プロジェクトの a")?;
    std::fs::write(src.join("a.txt"), "外の a")?;

    let out = add(
        &repo,
        &request(vec![src.join("a.txt")], "", AddConflictPolicy::KeepBoth),
    )?;
    assert_eq!(out.added.len(), 1);
    assert_eq!(out.added[0].path, "a (2).txt");
    assert!(out.added[0].renamed);
    assert!(!out.added[0].replaced);
    assert!(out.undo_ref.is_none());
    assert_eq!(read(&repo, "a.txt"), "プロジェクトの a");
    assert_eq!(read(&repo, "a (2).txt"), "外の a");
    Ok(())
}

#[test]
fn replacing_a_tracked_file_keeps_the_old_content_in_the_restore_point() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "報告 書.txt", "古い版")?;
    ops().save(&repo, "first")?;
    // 保存後に編集した内容（未保存）も、置き換え前に復元点へ入る
    write_test_file(&repo, "報告 書.txt", "編集中の版")?;
    std::fs::write(src.join("報告 書.txt"), "新しい版")?;
    let head_before = get_head_commit(&repo);

    let out = add(
        &repo,
        &request(
            vec![src.join("報告 書.txt")],
            "",
            AddConflictPolicy::Replace,
        ),
    )?;

    assert_eq!(out.added.len(), 1);
    assert!(out.added[0].replaced);
    assert!(!out.added[0].renamed);
    assert_eq!(out.added[0].path, "報告 書.txt");
    assert_eq!(read(&repo, "報告 書.txt"), "新しい版");
    // 保存（HEAD）は増えない。復元点が作られている
    assert_eq!(get_head_commit(&repo), head_before);
    assert_eq!(backup_count(&repo), 1);
    // 取り消し用の復元点には、置き換え前の内容が入っている
    let undo_ref = out.undo_ref.ok_or("undo_ref is missing")?;
    assert!(undo_ref.starts_with("refs/hikae/snapshots/"));
    assert_eq!(
        git_out(
            &repo,
            &["cat-file", "-p", &format!("{undo_ref}:報告 書.txt")]
        ),
        "編集中の版"
    );
    // 一時ファイルは残らない
    assert!(std::fs::read_dir(&repo)?
        .filter_map(Result::ok)
        .all(|e| !e.file_name().to_string_lossy().contains("hikae-tmp")));

    // 取り消すと、置き換え前の内容に戻る
    ops().undo_restore(&repo, &undo_ref)?;
    assert_eq!(read(&repo, "報告 書.txt"), "編集中の版");
    Ok(())
}

#[test]
fn replacing_an_unchanged_tracked_file_uses_head_as_the_backup() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "a.txt", "保存済みの版")?;
    ops().save(&repo, "first")?;
    std::fs::write(src.join("a.txt"), "新しい版")?;

    // 未保存の変更が無いと復元点（自動保存）は作られないが、HEAD に同じ内容がある
    let out = add(
        &repo,
        &request(vec![src.join("a.txt")], "", AddConflictPolicy::Replace),
    )?;
    assert!(out.added[0].replaced);
    assert_eq!(read(&repo, "a.txt"), "新しい版");
    let undo_ref = out.undo_ref.ok_or("undo_ref is missing")?;
    ops().undo_restore(&repo, &undo_ref)?;
    assert_eq!(read(&repo, "a.txt"), "保存済みの版");
    Ok(())
}

#[test]
fn replacing_an_untracked_file_needs_it_to_be_in_the_snapshot() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "未保存.txt", "まだ保存していない内容")?;
    std::fs::write(src.join("未保存.txt"), "外の版")?;

    let out = add(
        &repo,
        &request(vec![src.join("未保存.txt")], "", AddConflictPolicy::Replace),
    )?;
    assert!(out.added[0].replaced);
    assert_eq!(read(&repo, "未保存.txt"), "外の版");
    let undo_ref = out.undo_ref.ok_or("undo_ref is missing")?;
    assert_eq!(
        git_out(
            &repo,
            &["cat-file", "-p", &format!("{undo_ref}:未保存.txt")]
        ),
        "まだ保存していない内容"
    );
    Ok(())
}

#[test]
fn a_file_the_snapshot_cannot_hold_is_not_replaced() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    // 保存対象外のファイルは復元点に入らない
    write_test_file(&repo, ".gitignore", "ignored.txt\n")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "ignored.txt", "保存対象外の内容")?;
    // 警告閾値（このテストでは 1KB）を超える大きいファイル
    std::fs::write(repo.join("large.bin"), vec![7u8; 2048])?;
    std::fs::write(src.join("ignored.txt"), "外の版")?;
    std::fs::write(src.join("large.bin"), "小さい版")?;

    // 確認の一覧では、置き換えを選べるかが分かる
    let asked = add(
        &repo,
        &request(
            vec![src.join("ignored.txt"), src.join("large.bin")],
            "",
            AddConflictPolicy::Ask,
        ),
    )?;
    let can: Vec<(&str, bool)> = asked
        .needs_decision
        .iter()
        .map(|c| (c.path.as_str(), c.can_replace))
        .collect();
    assert!(can.contains(&("large.bin", false)));
    assert!(can.contains(&("ignored.txt", true)));

    // 置き換えを選んでも、上書きせずに別名にする
    let out = add(
        &repo,
        &request(
            vec![src.join("ignored.txt"), src.join("large.bin")],
            "",
            AddConflictPolicy::Replace,
        ),
    )?;
    assert_eq!(out.added.len(), 2);
    for file in &out.added {
        assert!(!file.replaced, "{file:?}");
        assert!(file.renamed, "{file:?}");
        assert!(file.replace_refused, "{file:?}");
    }
    assert!(out.undo_ref.is_none());
    assert_eq!(read(&repo, "ignored.txt"), "保存対象外の内容");
    assert_eq!(read(&repo, "ignored (2).txt"), "外の版");
    assert_eq!(std::fs::read(repo.join("large.bin"))?, vec![7u8; 2048]);
    assert_eq!(read(&repo, "large (2).bin"), "小さい版");
    Ok(())
}

#[test]
fn decisions_apply_per_file_and_skip_leaves_the_file_out() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    for name in ["r.txt", "k.txt", "s.txt"] {
        write_test_file(&repo, name, &format!("プロジェクトの {name}"))?;
        std::fs::write(src.join(name), format!("外の {name}"))?;
    }
    ops().save(&repo, "first")?;

    let mut req = request(
        vec![src.join("r.txt"), src.join("k.txt"), src.join("s.txt")],
        "",
        AddConflictPolicy::Ask,
    );
    req.decisions = vec![
        AddConflictDecision {
            path: "r.txt".into(),
            action: AddConflictAction::Replace,
        },
        AddConflictDecision {
            path: "k.txt".into(),
            action: AddConflictAction::KeepBoth,
        },
        AddConflictDecision {
            path: "s.txt".into(),
            action: AddConflictAction::Skip,
        },
    ];
    let out = add(&repo, &req)?;

    assert!(out.needs_decision.is_empty());
    assert_eq!(read(&repo, "r.txt"), "外の r.txt");
    assert_eq!(read(&repo, "k.txt"), "プロジェクトの k.txt");
    assert_eq!(read(&repo, "k (2).txt"), "外の k.txt");
    assert_eq!(read(&repo, "s.txt"), "プロジェクトの s.txt");
    assert!(!repo.join("s (2).txt").exists());
    assert_eq!(out.added.len(), 2);
    Ok(())
}

#[test]
fn a_folder_is_copied_recursively_and_hidden_or_temporary_files_are_skipped() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    let folder = src.join("資料");
    std::fs::create_dir_all(folder.join("図/下書き"))?;
    std::fs::write(folder.join("a.txt"), "a")?;
    std::fs::write(folder.join("図/b.txt"), "b")?;
    std::fs::write(folder.join("図/下書き/c.txt"), "c")?;
    std::fs::write(folder.join(".DS_Store"), "x")?;
    std::fs::write(folder.join("Thumbs.db"), "x")?;
    std::fs::write(folder.join("~$報告書.docx"), "x")?;
    std::fs::write(folder.join(".hidden"), "x")?;
    std::fs::create_dir_all(folder.join(".secret"))?;
    std::fs::write(folder.join(".secret/inner.txt"), "x")?;
    // 空のフォルダはコピーしない（ファイルが無いため）
    std::fs::create_dir_all(folder.join("空"))?;
    // 大きいファイルはフォルダの中でも 1 件ずつ検査する（このテストの閾値は 1KB / 10KB）
    std::fs::write(folder.join("大きめ.bin"), vec![1u8; 2048])?;
    std::fs::write(folder.join("巨大.bin"), vec![1u8; 20 * 1024])?;

    let out = add(
        &repo,
        &request(vec![folder.clone()], "", AddConflictPolicy::Ask),
    )?;

    assert_eq!(read(&repo, "資料/a.txt"), "a");
    assert_eq!(read(&repo, "資料/図/b.txt"), "b");
    assert_eq!(read(&repo, "資料/図/下書き/c.txt"), "c");
    assert!(repo.join("資料/大きめ.bin").exists());
    assert!(!repo.join("資料/巨大.bin").exists());
    for skipped in [
        ".DS_Store",
        "Thumbs.db",
        "~$報告書.docx",
        ".hidden",
        ".secret",
    ] {
        assert!(!repo.join("資料").join(skipped).exists(), "{skipped}");
    }
    assert!(!repo.join("資料/空").exists());
    // 結果: 追加したもの、大きいもの、断ったもの、飛ばしたもの
    let mut added: Vec<&str> = out.added.iter().map(|f| f.path.as_str()).collect();
    added.sort();
    assert_eq!(
        added,
        vec![
            "資料/a.txt",
            "資料/図/b.txt",
            "資料/図/下書き/c.txt",
            "資料/大きめ.bin"
        ]
    );
    assert!(
        out.added
            .iter()
            .find(|f| f.path == "資料/大きめ.bin")
            .ok_or("missing")?
            .large
    );
    assert_eq!(out.rejected.len(), 1);
    assert_eq!(out.rejected[0].name, "資料/巨大.bin");
    assert!(matches!(
        out.rejected[0].reason,
        AddRejectReason::TooLarge { size } if size == 20 * 1024
    ));
    let reason_of = |name: &str| {
        out.skipped
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.reason)
    };
    assert_eq!(reason_of("資料/.DS_Store"), Some(SkipReason::OsTemp));
    assert_eq!(reason_of("資料/Thumbs.db"), Some(SkipReason::OsTemp));
    assert_eq!(reason_of("資料/~$報告書.docx"), Some(SkipReason::OsTemp));
    assert_eq!(reason_of("資料/.hidden"), Some(SkipReason::Hidden));
    assert_eq!(reason_of("資料/.secret"), Some(SkipReason::Hidden));
    // コピー元は動かさない
    assert!(folder.join("a.txt").exists());
    Ok(())
}

#[test]
fn folder_files_that_already_exist_follow_the_conflict_policy() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "資料/a.txt", "プロジェクトの a")?;
    ops().save(&repo, "first")?;
    let folder = src.join("資料");
    std::fs::create_dir_all(folder.join("sub"))?;
    std::fs::write(folder.join("a.txt"), "外の a")?;
    std::fs::write(folder.join("sub/n.txt"), "新しい n")?;

    // 既存のフォルダにまとめる。同名はファイルごとに確認する（フォルダ内のパスで示す）
    let asked = add(
        &repo,
        &request(vec![folder.clone()], "", AddConflictPolicy::Ask),
    )?;
    assert_eq!(asked.needs_decision.len(), 1);
    assert_eq!(asked.needs_decision[0].path, "資料/a.txt");
    assert!(!repo.join("資料/sub").exists());

    // フォルダ全体に同じ方針（すべてに適用）
    let out = add(
        &repo,
        &request(vec![folder.clone()], "", AddConflictPolicy::KeepBoth),
    )?;
    assert_eq!(read(&repo, "資料/a.txt"), "プロジェクトの a");
    assert_eq!(read(&repo, "資料/a (2).txt"), "外の a");
    assert_eq!(read(&repo, "資料/sub/n.txt"), "新しい n");
    assert_eq!(out.added.len(), 2);
    assert!(out.undo_ref.is_none());
    Ok(())
}

#[test]
fn a_folder_never_overwrites_a_file_with_the_same_name() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "資料", "これはファイル")?;
    let folder = src.join("資料");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("a.txt"), "a")?;

    let out = add(&repo, &request(vec![folder], "", AddConflictPolicy::Ask))?;
    assert_eq!(read(&repo, "資料"), "これはファイル");
    assert_eq!(read(&repo, "資料 (2)/a.txt"), "a");
    assert_eq!(out.added[0].path, "資料 (2)/a.txt");
    Ok(())
}

#[test]
fn too_many_files_or_too_deep_folders_are_refused_before_anything_is_written() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;

    // 1,000 件ちょうどは受け付け、1,001 件は何もコピーせずに断る
    let many = src.join("多い");
    std::fs::create_dir_all(&many)?;
    for i in 0..1000 {
        std::fs::write(many.join(format!("{i}.txt")), "x")?;
    }
    let ok = add(
        &repo,
        &request(vec![many.clone()], "", AddConflictPolicy::KeepBoth),
    )?;
    assert_eq!(ok.added.len(), 1000);
    std::fs::remove_dir_all(repo.join("多い"))?;

    std::fs::write(many.join("1000.txt"), "x")?;
    let before_refs = git_out(&repo, &["for-each-ref", "refs/hikae/"]);
    let err = add(&repo, &request(vec![many], "", AddConflictPolicy::KeepBoth))
        .expect_err("too many files");
    assert!(matches!(
        err,
        OpsError::AddRefused(AddRefusal::TooManyFiles { limit: 1000 })
    ));
    assert!(!repo.join("多い").exists());
    assert_eq!(
        git_out(&repo, &["for-each-ref", "refs/hikae/"]),
        before_refs
    );

    // 階層は 20 まで（ドロップしたフォルダの中の階層）
    let mut deep = src.join("深い");
    std::fs::create_dir_all(&deep)?;
    let ok_root = deep.clone();
    for i in 0..20 {
        deep = deep.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&deep)?;
    std::fs::write(deep.join("leaf.txt"), "x")?;
    let ok = add(
        &repo,
        &request(vec![ok_root.clone()], "", AddConflictPolicy::KeepBoth),
    )?;
    assert_eq!(ok.added.len(), 1);
    std::fs::remove_dir_all(repo.join("深い"))?;

    let deeper = deep.join("d20");
    std::fs::create_dir_all(&deeper)?;
    std::fs::write(deeper.join("leaf.txt"), "x")?;
    let err = add(
        &repo,
        &request(vec![ok_root], "", AddConflictPolicy::KeepBoth),
    )
    .expect_err("too deep");
    assert!(matches!(
        err,
        OpsError::AddRefused(AddRefusal::TooDeep { limit: 20 })
    ));
    assert!(!repo.join("深い").exists());
    Ok(())
}

#[test]
fn links_are_skipped_and_never_followed() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    let secret = tmp.path().join("secret");
    std::fs::create_dir_all(&secret)?;
    std::fs::write(secret.join("password.txt"), "秘密")?;
    let folder = src.join("リンク入り");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("ok.txt"), "ok")?;

    // リンクを作れない環境（権限の無い Windows など）では、リンクの部分だけ確認を省く
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&secret, folder.join("外へのリンク")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&secret, folder.join("外へのリンク")).is_ok();
    #[cfg(not(any(unix, windows)))]
    let linked = false;

    let out = add(&repo, &request(vec![folder], "", AddConflictPolicy::Ask))?;
    assert_eq!(read(&repo, "リンク入り/ok.txt"), "ok");
    assert!(!repo.join("リンク入り/外へのリンク").exists());
    assert!(!repo.join("リンク入り/外へのリンク/password.txt").exists());
    if linked {
        assert!(out
            .skipped
            .iter()
            .any(|s| s.name == "リンク入り/外へのリンク" && s.reason == SkipReason::Link));
    }
    Ok(())
}

#[test]
fn destination_subfolder_and_state_are_respected() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let src = source_dir(tmp.path())?;
    write_test_file(&repo, "docs/keep.txt", "keep")?;
    ops().save(&repo, "first")?;
    let head = get_head_commit(&repo);
    let index_before = git_out(&repo, &["ls-files", "-s"]);
    let folder = src.join("図");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("1.png"), "png")?;

    // 追加先は、プロジェクト内のフォルダ（ツリー上のフォルダへのドロップ）
    let out = add(
        &repo,
        &request(vec![folder], "docs", AddConflictPolicy::Ask),
    )?;
    assert_eq!(out.added[0].path, "docs/図/1.png");
    assert_eq!(read(&repo, "docs/図/1.png"), "png");
    // 保存はしない。インデックスも変えない。復元点は作られる
    assert_eq!(get_head_commit(&repo), head);
    assert_eq!(git_out(&repo, &["ls-files", "-s"]), index_before);
    assert_eq!(backup_count(&repo), 1);
    Ok(())
}
