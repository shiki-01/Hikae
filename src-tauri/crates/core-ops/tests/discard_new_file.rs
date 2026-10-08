// 新規ファイルの「元に戻す（作成しない）」（Ops::discard_new_file）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使い、ごみ箱は「一時フォルダへ移す実装」に差し替える。
// 不変条件 3（復元点）・4（作業フォルダとインデックスを変えない）・6（復元不能な削除をしない）の検証が目的。
// 時間計測は使わない。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{DiscardRefusal, Identity, Ops, OpsError, SizeLimits, Trasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// ごみ箱の代わりに、一時フォルダへ移す。`fail` なら何もせずエラーにする（ごみ箱が使えない環境）
struct MoveToDir {
    dir: PathBuf,
    fail: bool,
    moved: Mutex<Vec<PathBuf>>,
}

impl MoveToDir {
    fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("create trash dir");
        MoveToDir {
            dir,
            fail: false,
            moved: Mutex::new(Vec::new()),
        }
    }

    fn failing(dir: PathBuf) -> Self {
        MoveToDir {
            fail: true,
            ..MoveToDir::new(dir)
        }
    }

    fn count(&self) -> usize {
        self.moved.lock().expect("lock").len()
    }
}

impl Trasher for MoveToDir {
    fn trash(&self, path: &Path) -> io::Result<()> {
        if self.fail {
            return Err(io::Error::other("trash is not available"));
        }
        let mut moved = self.moved.lock().expect("lock");
        let name = path.file_name().expect("file name").to_owned();
        let dest = self
            .dir
            .join(format!("{}-{}", moved.len(), name.to_string_lossy()));
        std::fs::rename(path, &dest)?;
        moved.push(dest);
        Ok(())
    }
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

fn refs(repo: &Path) -> String {
    git_out(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/hikae/",
        ],
    )
}

fn refusal(error: OpsError) -> Option<DiscardRefusal> {
    match error {
        OpsError::DiscardRefused { reason, .. } => Some(reason),
        _ => None,
    }
}

#[test]
fn untracked_file_goes_to_the_trash_after_a_snapshot_holds_its_content() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "keep.txt", "残すファイル")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "keep.txt", "編集中")?;
    write_test_file(&repo, "other new.txt", "別の新規ファイル")?;
    let target = "資料/新しい 版.txt";
    let content = "新規ファイルの内容\n二行目";
    write_test_file(&repo, target, content)?;

    let head_before = get_head_commit(&repo);
    let index_before = git_out(&repo, &["ls-files", "-s"]);
    let status_before = git_out(&repo, &["status", "--porcelain=v2", "-z", "-uall"]);
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let done = ops().discard_new_file(&repo, target, LIMITS, &trash)?;

    // ごみ箱へ移った（元の場所には無い。内容はそのまま）
    assert!(!repo.join(target).exists());
    assert_eq!(trash.count(), 1);
    let moved = std::fs::read_to_string(&trash.moved.lock().expect("lock")[0])?;
    assert_eq!(moved, content);

    // 復元点に内容が入っている
    let undo_ref = done.undo_ref.ok_or("undo_ref is missing")?;
    assert!(undo_ref.starts_with("refs/hikae/snapshots/"));
    let stored = git_out(&repo, &["cat-file", "-p", &format!("{undo_ref}:{target}")]);
    assert_eq!(stored, content);
    assert!(refs(&repo).contains("refs/hikae/backup/discard-new-file/"));

    // ほかのファイル・インデックス・保存（HEAD）は変わらない
    assert_eq!(get_head_commit(&repo), head_before);
    assert_eq!(git_out(&repo, &["ls-files", "-s"]), index_before);
    assert_eq!(std::fs::read_to_string(repo.join("keep.txt"))?, "編集中");
    assert_eq!(
        std::fs::read_to_string(repo.join("other new.txt"))?,
        "別の新規ファイル"
    );
    // 状態の変化は、対象のファイルが一覧から消えただけ
    let status_after = git_out(&repo, &["status", "--porcelain=v2", "-z", "-uall"]);
    assert!(status_before.contains("新しい 版.txt"));
    assert!(!status_after.contains("新しい 版.txt"));
    assert_eq!(
        status_before.replace(&format!("? {target}\0"), ""),
        status_after
    );
    Ok(())
}

#[test]
fn the_file_comes_back_with_undo_restore() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "新規.txt", "取り消せる内容")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let done = ops().discard_new_file(&repo, "新規.txt", LIMITS, &trash)?;
    assert!(!repo.join("新規.txt").exists());

    ops().undo_restore(&repo, &done.undo_ref.ok_or("undo_ref is missing")?)?;
    assert_eq!(
        std::fs::read_to_string(repo.join("新規.txt"))?,
        "取り消せる内容"
    );
    // 取り消しは復元点の復元（インデックスにも登録する）。戻ったファイルは新規ファイルとして一覧に出るが、
    // 未追跡ではなくなる（設計書 4.2）
    let back = ops().list_changes(&repo)?;
    let file = back
        .iter()
        .find(|c| c.path == "新規.txt")
        .ok_or("the file is not listed")?;
    assert!(!file.untracked);
    Ok(())
}

#[test]
fn an_empty_file_can_be_discarded() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "空.txt", "")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let done = ops().discard_new_file(&repo, "空.txt", LIMITS, &trash)?;
    assert!(!repo.join("空.txt").exists());
    let undo_ref = done.undo_ref.ok_or("undo_ref is missing")?;
    assert_eq!(
        git_out(&repo, &["cat-file", "-s", &format!("{undo_ref}:空.txt")]).trim(),
        "0"
    );
    Ok(())
}

#[test]
fn works_when_an_identical_automatic_snapshot_already_exists() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "新規.txt", "自動保存済みの内容")?;
    // 先に自動保存が同じ内容で作られている（復元点が重複を作らないケース）
    ops().auto_snapshot(&repo, LIMITS)?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let done = ops().discard_new_file(&repo, "新規.txt", LIMITS, &trash)?;
    assert!(!repo.join("新規.txt").exists());
    let undo_ref = done.undo_ref.ok_or("undo_ref is missing")?;
    let stored = git_out(&repo, &["cat-file", "-p", &format!("{undo_ref}:新規.txt")]);
    assert_eq!(stored, "自動保存済みの内容");
    Ok(())
}

#[test]
fn tracked_files_are_refused_and_stay() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "saved.txt", "saved")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "saved.txt", "edited")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));
    let refs_before = refs(&repo);

    let err = ops()
        .discard_new_file(&repo, "saved.txt", LIMITS, &trash)
        .err()
        .ok_or("must be refused")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::NotUntracked));
    assert_eq!(std::fs::read_to_string(repo.join("saved.txt"))?, "edited");
    assert_eq!(trash.count(), 0);
    // 拒否したときは復元点も作らない
    assert_eq!(refs(&repo), refs_before);
    Ok(())
}

#[test]
fn files_already_added_to_the_index_are_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "staged.txt", "s")?;
    git_out(&repo, &["add", "staged.txt"]);
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let err = ops()
        .discard_new_file(&repo, "staged.txt", LIMITS, &trash)
        .err()
        .ok_or("must be refused")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::NotUntracked));
    assert!(repo.join("staged.txt").exists());
    assert_eq!(trash.count(), 0);
    Ok(())
}

#[test]
fn files_excluded_from_saving_are_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, ".gitignore", "*.log\n")?;
    write_test_file(&repo, "debug.log", "ログ")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    // 保存対象外のファイルは復元点に入らないため、削除しない
    let err = ops()
        .discard_new_file(&repo, "debug.log", LIMITS, &trash)
        .err()
        .ok_or("must be refused")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::NotUntracked));
    assert!(repo.join("debug.log").exists());
    Ok(())
}

#[test]
fn folders_and_missing_files_are_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "資料/中.txt", "inside")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    for target in ["資料", "no-such-file.txt", "資料/無い.txt"] {
        let err = ops()
            .discard_new_file(&repo, target, LIMITS, &trash)
            .err()
            .ok_or("must be refused")?;
        assert_eq!(refusal(err), Some(DiscardRefusal::NotAFile), "{target}");
    }
    assert!(repo.join("資料/中.txt").exists());
    assert_eq!(trash.count(), 0);
    Ok(())
}

#[test]
fn paths_outside_the_project_are_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "プロジェクトの外")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    for target in [
        "../outside.txt",
        "sub/../../outside.txt",
        "..\\outside.txt",
        outside.to_string_lossy().as_ref(),
        ".git/config",
        "",
    ] {
        let err = ops()
            .discard_new_file(&repo, target, LIMITS, &trash)
            .err()
            .ok_or("must be refused")?;
        assert!(
            matches!(err, OpsError::InvalidInput(_)),
            "{target:?} must be invalid input: {err:?}"
        );
    }
    assert!(outside.exists());
    assert_eq!(trash.count(), 0);
    Ok(())
}

#[test]
fn symlinks_are_refused_and_never_followed() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "外のファイル")?;
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, repo.join("link.txt")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside, repo.join("link.txt")).is_ok();
    if !linked {
        eprintln!("skip: symlink を作成できない環境");
        return Ok(());
    }
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let err = ops()
        .discard_new_file(&repo, "link.txt", LIMITS, &trash)
        .err()
        .ok_or("must be refused")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::NotAFile));
    assert!(outside.exists());
    assert!(std::fs::symlink_metadata(repo.join("link.txt")).is_ok());
    assert_eq!(trash.count(), 0);
    Ok(())
}

#[test]
fn files_through_a_symlinked_folder_are_refused() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    let outside_dir = tmp.path().join("outside");
    std::fs::create_dir_all(&outside_dir)?;
    std::fs::write(outside_dir.join("x.txt"), "外")?;
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside_dir, repo.join("up")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outside_dir, repo.join("up")).is_ok();
    if !linked {
        eprintln!("skip: symlink を作成できない環境");
        return Ok(());
    }
    let trash = MoveToDir::new(tmp.path().join("trash"));

    assert!(ops()
        .discard_new_file(&repo, "up/x.txt", LIMITS, &trash)
        .is_err());
    assert!(outside_dir.join("x.txt").exists());
    assert_eq!(trash.count(), 0);
    Ok(())
}

#[test]
fn files_too_large_for_a_snapshot_are_refused_without_touching_anything() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    // 閾値ちょうどは対象、超えるものは拒否
    write_test_file(&repo, "ちょうど.bin", &"a".repeat(1024))?;
    write_test_file(&repo, "大きい.bin", &"a".repeat(1025))?;
    let trash = MoveToDir::new(tmp.path().join("trash"));
    let refs_before = refs(&repo);

    let err = ops()
        .discard_new_file(&repo, "大きい.bin", LIMITS, &trash)
        .err()
        .ok_or("must be refused")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::TooLarge { size: 1025 }));
    assert!(repo.join("大きい.bin").exists());
    assert_eq!(trash.count(), 0);
    assert_eq!(refs(&repo), refs_before);

    ops().discard_new_file(&repo, "ちょうど.bin", LIMITS, &trash)?;
    assert!(!repo.join("ちょうど.bin").exists());
    Ok(())
}

#[test]
fn the_file_stays_when_the_trash_is_not_available() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "新規.txt", "消さない")?;
    let trash = MoveToDir::failing(tmp.path().join("trash"));

    let err = ops()
        .discard_new_file(&repo, "新規.txt", LIMITS, &trash)
        .err()
        .ok_or("must fail")?;
    assert_eq!(refusal(err), Some(DiscardRefusal::TrashFailed));
    assert_eq!(std::fs::read_to_string(repo.join("新規.txt"))?, "消さない");
    Ok(())
}

#[test]
fn only_the_requested_file_moves_when_several_new_files_exist() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "a.txt", "a")?;
    write_test_file(&repo, "b.txt", "b")?;
    write_test_file(&repo, "dir/c.txt", "c")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    ops().discard_new_file(&repo, "b.txt", LIMITS, &trash)?;
    assert!(repo.join("a.txt").exists());
    assert!(!repo.join("b.txt").exists());
    assert!(repo.join("dir/c.txt").exists());
    assert_eq!(trash.count(), 1);
    Ok(())
}

#[test]
fn works_in_a_project_that_has_no_save_yet() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "最初のファイル.txt", "まだ何も保存していない")?;
    let trash = MoveToDir::new(tmp.path().join("trash"));

    let done = ops().discard_new_file(&repo, "最初のファイル.txt", LIMITS, &trash)?;
    assert!(!repo.join("最初のファイル.txt").exists());
    let undo_ref = done.undo_ref.ok_or("undo_ref is missing")?;
    let stored = git_out(
        &repo,
        &["cat-file", "-p", &format!("{undo_ref}:最初のファイル.txt")],
    );
    assert_eq!(stored, "まだ何も保存していない");
    Ok(())
}
