// 初期の除外設定（Ops::apply_default_excludes。設計書 13.1）の統合テスト。
// 実 git と一時ディレクトリの実リポジトリを使う。Office の一時ファイルが変更一覧に出ないこと、
// 通常のファイルは出ること、既存の除外設定が保たれること、作業フォルダ・インデックス・`.gitignore` に
// 触れないこと、登録を外して再登録しても `.git` と保存先が引き継がれることを確かめる。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{ExcludeOutcome, Identity, Ops, EXCLUDE_BLOCK_BEGIN};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env()).with_pc_name(None)
}

fn identity() -> Identity {
    Identity {
        name: "tester".to_string(),
        email: "tester@users.noreply.github.com".to_string(),
    }
}

fn new_repo(tmp: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(&repo, None, &identity())?;
    Ok(repo)
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout
}

fn exclude_path(repo: &Path) -> PathBuf {
    repo.join(".git").join("info").join("exclude")
}

fn listed(repo: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut paths: Vec<String> = ops()
        .list_changes(repo)?
        .into_iter()
        .map(|c| c.path)
        .collect();
    paths.sort();
    Ok(paths)
}

#[test]
fn office_temporary_files_stay_out_of_the_change_list() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "~$報告書.docx", "owner")?;
    write_test_file(&repo, "資料/~$表.xlsx", "owner")?;
    write_test_file(&repo, ".~lock.表.ods#", "lock")?;
    write_test_file(&repo, "~WRL0001.tmp", "tmp")?;
    write_test_file(&repo, ".DS_Store", "x")?;
    write_test_file(&repo, "Thumbs.db", "x")?;
    write_test_file(&repo, "desktop.ini", "x")?;
    write_test_file(&repo, "報告書.docx", "本体")?;
    // 広いパターンは入れていないので、通常の *.tmp は変更として出る
    write_test_file(&repo, "作業中.tmp", "keep me")?;
    let before = listed(&repo)?;
    assert!(before.contains(&"~$報告書.docx".to_string()));

    let worktree_before = git_out(&repo, &["ls-files", "-s"]);
    let outcome = ops().apply_default_excludes(&repo, false)?;
    assert_eq!(outcome, ExcludeOutcome::Written);

    assert_eq!(
        listed(&repo)?,
        vec!["作業中.tmp".to_string(), "報告書.docx".to_string()]
    );
    // 作業フォルダのファイルはそのまま。インデックスも、`.gitignore` も作られない
    assert_eq!(
        std::fs::read_to_string(repo.join("~$報告書.docx"))?,
        "owner"
    );
    assert_eq!(git_out(&repo, &["ls-files", "-s"]), worktree_before);
    assert!(!repo.join(".gitignore").exists());
    // 保存すると、一時ファイルは含まれない
    ops().save(&repo, "second")?;
    let tracked = git_out(&repo, &["ls-files", "-z"]);
    assert!(tracked.contains("報告書.docx"));
    assert!(!tracked.contains("~$"));
    assert!(!tracked.contains("Thumbs.db"));
    Ok(())
}

#[test]
fn an_existing_exclude_file_is_kept_and_nothing_is_duplicated() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;
    let path = exclude_path(&repo);
    std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    std::fs::write(&path, "# 自分の設定\n*.log\nThumbs.db")?;

    ops().apply_default_excludes(&repo, false)?;
    let after = std::fs::read_to_string(&path)?;
    assert!(after.starts_with("# 自分の設定\n*.log\nThumbs.db\n"));
    assert_eq!(after.lines().filter(|l| *l == "Thumbs.db").count(), 1);
    assert_eq!(after.matches(EXCLUDE_BLOCK_BEGIN).count(), 1);
    assert!(after.contains("~$*"));

    // もう一度適用しても変わらない
    assert_eq!(
        ops().apply_default_excludes(&repo, false)?,
        ExcludeOutcome::Unchanged
    );
    assert_eq!(std::fs::read_to_string(&path)?, after);

    // 利用者の除外（*.log）は引き続き効く
    write_test_file(&repo, "debug.log", "log")?;
    write_test_file(&repo, "~$a.docx", "owner")?;
    assert!(listed(&repo)?.is_empty());
    Ok(())
}

#[test]
fn startup_application_happens_once_and_respects_a_removed_block() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    write_test_file(&repo, "base.txt", "base")?;
    ops().save(&repo, "first")?;

    // 起動時の適用（マーカーが無い場合のみ）
    assert_eq!(
        ops().apply_default_excludes(&repo, true)?,
        ExcludeOutcome::Written
    );
    let once = std::fs::read_to_string(exclude_path(&repo))?;
    assert_eq!(
        ops().apply_default_excludes(&repo, true)?,
        ExcludeOutcome::Unchanged
    );

    // 管理する部分の中身を利用者が編集していても、起動時の適用は書き換えない
    let edited = once.replace("desktop.ini\n", "");
    std::fs::write(exclude_path(&repo), &edited)?;
    assert_eq!(
        ops().apply_default_excludes(&repo, true)?,
        ExcludeOutcome::Unchanged
    );
    assert_eq!(std::fs::read_to_string(exclude_path(&repo))?, edited);
    Ok(())
}

#[test]
fn a_restore_point_is_made_before_the_change_when_there_is_something_to_protect() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let repo = new_repo(tmp.path())?;
    // 保存もファイルも無い新しいフォルダでは、復元点は作らない
    ops().apply_default_excludes(&repo, false)?;
    assert!(git_out(&repo, &["for-each-ref", "refs/hikae/"]).is_empty());

    // 保存があるプロジェクトでは、変更の前に復元点（backup）を作る
    let other = tmp.path().join("other");
    ops().init_project(&other, None, &identity())?;
    write_test_file(&other, "base.txt", "base")?;
    ops().save(&other, "first")?;
    write_test_file(&other, "未保存.txt", "編集中")?;
    ops().apply_default_excludes(&other, false)?;
    let refs = git_out(
        &other,
        &["for-each-ref", "--format=%(refname)", "refs/hikae/"],
    );
    assert!(refs.contains("refs/hikae/backup/set-excludes/"));
    assert!(refs.contains("refs/hikae/snapshots/"));
    // 未保存のファイルは、そのまま残っている
    assert_eq!(std::fs::read_to_string(other.join("未保存.txt"))?, "編集中");
    Ok(())
}

#[test]
fn registering_the_same_folder_again_keeps_history_remote_and_excludes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    create_bare_repo(&bare)?;
    let repo = new_repo(tmp.path())?;
    let url = bare.to_string_lossy().to_string();
    git_out(&repo, &["remote", "add", "origin", &url]);
    write_test_file(&repo, "報告書.txt", "本文")?;
    ops().save(&repo, "first")?;
    write_test_file(&repo, "編集中.txt", "未保存")?;
    ops().apply_default_excludes(&repo, false)?;
    // 復元点（自動保存）が履歴の外に残っている
    let refs_before = git_out(
        &repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/hikae/",
        ],
    );
    assert!(!refs_before.is_empty());
    let head = get_head_commit(&repo);
    let exclude = std::fs::read_to_string(exclude_path(&repo))?;

    // 「一覧から外す」の後に、同じフォルダを登録し直す流れ（アプリは init_project を通す）
    ops().init_project(&repo, None, &identity())?;

    assert_eq!(ops().origin_url(&repo)?.as_deref(), Some(url.as_str()));
    assert_eq!(get_head_commit(&repo), head);
    assert_eq!(ops().initial_commit(&repo)?, ops().initial_commit(&repo)?);
    assert!(ops().initial_commit(&repo)?.is_some());
    assert_eq!(
        git_out(
            &repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/hikae/"
            ]
        ),
        refs_before
    );
    assert_eq!(std::fs::read_to_string(exclude_path(&repo))?, exclude);
    assert_eq!(std::fs::read_to_string(repo.join("編集中.txt"))?, "未保存");
    // 除外設定を重ねて適用しても、重複しない
    assert_eq!(
        ops().apply_default_excludes(&repo, false)?,
        ExcludeOutcome::Unchanged
    );
    Ok(())
}
