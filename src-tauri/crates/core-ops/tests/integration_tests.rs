// 統合テスト：実 git を使用。tempfile で一時ディレクトリを作成。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{Choice, Ops};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use time::OffsetDateTime;

/// テストの共通セットアップ：bare リポジトリ、PC-A clone、PC-B clone
struct TestSetup {
    _tmp: tempfile::TempDir,
    bare_repo: PathBuf,
    pc_a: PathBuf,
    pc_b: PathBuf,
}

impl TestSetup {
    fn new(test_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // 一時ディレクトリ（絶対パス）。Drop で削除される
        let tmp = tempfile::Builder::new().prefix(test_name).tempdir()?;
        let test_dir = tmp.path().to_path_buf();

        let bare_repo = test_dir.join("remote.git");
        let pc_a = test_dir.join("pc-a");
        let pc_b = test_dir.join("pc-b");

        // bare リポジトリ作成
        create_bare_repo(&bare_repo)?;

        // PC-A init
        let clock_a = Arc::new(AtomicU64::new(1000000));
        let ops_a = make_ops_with_clock(&new_runner(), &clock_a);
        let bare_url = bare_repo.to_string_lossy().to_string();
        ops_a.init_project(
            &pc_a,
            Some(&bare_url),
            &core_ops::Identity {
                name: "pc-a".to_string(),
                email: "pc-a@users.noreply.github.com".to_string(),
            },
        )?;

        // PC-A 初回 save
        common::write_test_file(&pc_a, "file.txt", "initial content")?;
        ops_a.save(&pc_a, "initial commit")?;

        // PC-A 初回 upload
        ops_a.upload(&pc_a)?;

        // PC-B clone
        let clock_b = Arc::new(AtomicU64::new(2000000));
        let ops_b = make_ops_with_clock(&new_runner(), &clock_b);
        ops_b.clone_project(
            &bare_url,
            &pc_b,
            &core_ops::Identity {
                name: "pc-b".to_string(),
                email: "pc-b@users.noreply.github.com".to_string(),
            },
        )?;

        Ok(TestSetup {
            _tmp: tmp,
            bare_repo,
            pc_a,
            pc_b,
        })
    }

    fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let parent = self.bare_repo.parent().ok_or("no parent")?;
        std::fs::remove_dir_all(parent)?;
        Ok(())
    }
}

fn make_ops_with_clock(_runner: &GitRunner, clock: &Arc<AtomicU64>) -> Ops {
    let clock_clone = Arc::clone(clock);
    Ops::new(new_runner()).with_clock(move || {
        let nanos = clock_clone.load(Ordering::SeqCst);
        OffsetDateTime::from_unix_timestamp_nanos(nanos as i128)
            .unwrap_or(OffsetDateTime::now_utc())
    })
}

// 新しい GitRunner を作成（テスト用）
fn new_runner() -> GitRunner {
    GitRunner::from_path_env()
}

#[test]
fn test_1_save_upload_pull_no_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-1")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // A が memo.txt を作成して save
    common::write_test_file(&setup.pc_a, "memo.txt", "memo from A")?;
    ops_a.save(&setup.pc_a, "add memo from A")?;

    // A が upload
    ops_a.upload(&setup.pc_a)?;

    // B で pull
    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::FastForwarded => {}
        _ => panic!("expected FastForwarded"),
    }

    // B の作業フォル더에 내용이 있는지 확인
    let memo_path = setup.pc_b.join("memo.txt");
    assert!(memo_path.exists(), "memo.txt should exist in B");
    let content = std::fs::read_to_string(&memo_path)?;
    assert_eq!(content, "memo from A");

    // B で다른 파일을 수정
    common::write_test_file(&setup.pc_b, "other.txt", "other from B")?;
    ops_b.save(&setup.pc_b, "add other from B")?;

    // B가 upload
    ops_b.upload(&setup.pc_b)?;

    // A에서 pull
    match ops_a.pull(&setup.pc_a)? {
        core_ops::PullOutcome::FastForwarded => {}
        _ => panic!("expected FastForwarded"),
    }

    // A의 작업 폴더에 B의 변경사항이 있는지 확인
    let other_path = setup.pc_a.join("other.txt");
    assert!(other_path.exists(), "other.txt should exist in A");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_2_both_differ_no_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-2")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // A와 B가 다른 파일을 변경
    common::write_test_file(&setup.pc_a, "a-file.txt", "content from A")?;
    ops_a.save(&setup.pc_a, "A's change")?;
    ops_a.upload(&setup.pc_a)?;

    common::write_test_file(&setup.pc_b, "b-file.txt", "content from B")?;
    ops_b.save(&setup.pc_b, "B's change")?;

    // B의 upload (자동 pull + push)
    match ops_b.upload(&setup.pc_b)? {
        core_ops::UploadOutcome::PulledThenPushed(core_ops::PullOutcome::Merged { .. }) => {}
        other => panic!("expected PulledThenPushed(Merged), got {:?}", other),
    }

    // A에서 pull해서 B의 변경사항을 받음
    ops_a.pull(&setup.pc_a)?;

    // A의 작업 폴더에 B의 파일이 있는지 확인
    let b_file = setup.pc_a.join("b-file.txt");
    assert!(b_file.exists(), "b-file.txt should exist in A");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_3_conflict_mine() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-3")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // A와 B가 같은 파일의 같은 줄을 다르게 변경
    common::write_test_file(&setup.pc_a, "conflict.txt", "line1\nline2\nline3")?;
    ops_a.save(&setup.pc_a, "initial conflict file")?;
    ops_a.upload(&setup.pc_a)?;

    // B에서 pull
    ops_b.pull(&setup.pc_b)?;

    // A에서 변경
    common::write_test_file(&setup.pc_a, "conflict.txt", "line1 from A\nline2\nline3")?;
    ops_a.save(&setup.pc_a, "A's modification")?;
    ops_a.upload(&setup.pc_a)?;

    // B에서 같은 라인 변경
    common::write_test_file(&setup.pc_b, "conflict.txt", "line1 from B\nline2\nline3")?;
    ops_b.save(&setup.pc_b, "B's modification")?;

    // B에서 pull -> 경합
    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::Conflicted { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "conflict.txt");
            assert_eq!(files[0].kind, core_ops::ConflictKind::BothModified);
            // 競合時にこの PC 側とクラウド側の最終保存日時が取得できる
            assert!(files[0].this_saved_at.is_some_and(|t| t > 0));
            assert!(files[0].cloud_saved_at.is_some_and(|t| t > 0));
        }
        _ => panic!("expected Conflicted"),
    }

    // conflicts() でも同じ日時が取得できる
    let listed = ops_b.conflicts(&setup.pc_b)?;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].this_saved_at.is_some_and(|t| t > 0));
    assert!(listed[0].cloud_saved_at.is_some_and(|t| t > 0));

    // B에서 작업 폴더에 conflict marker가 있는지 확인
    let content = std::fs::read_to_string(setup.pc_b.join("conflict.txt"))?;
    assert!(content.contains("<<<<<<<"), "conflict marker should exist");

    // B에서 Mine을 선택해서 resolve
    ops_b.resolve(
        &setup.pc_b,
        &[("conflict.txt".to_string(), Choice::Mine)],
        false,
        "resolved: keeping B's version",
    )?;

    // B의 내용이 유지되는지 확인
    let resolved_content = std::fs::read_to_string(setup.pc_b.join("conflict.txt"))?;
    assert_eq!(resolved_content, "line1 from B\nline2\nline3");

    // B에서 upload
    ops_b.upload(&setup.pc_b)?;

    // A에서 pull
    ops_a.pull(&setup.pc_a)?;

    // A에도 B의 내용이 반영되는지 확인
    let a_content = std::fs::read_to_string(setup.pc_a.join("conflict.txt"))?;
    assert_eq!(a_content, "line1 from B\nline2\nline3");

    // conflict가 resolve됐는지 확인
    let conflicts = ops_b.conflicts(&setup.pc_b)?;
    assert!(conflicts.is_empty(), "no conflicts should remain");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_4_conflict_theirs_keep_copy() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-4")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 초기 파일 생성
    common::write_test_file(&setup.pc_a, "conflict.txt", "original")?;
    ops_a.save(&setup.pc_a, "initial")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;

    // A와 B가 다르게 변경
    common::write_test_file(&setup.pc_a, "conflict.txt", "version A")?;
    ops_a.save(&setup.pc_a, "A's version")?;
    ops_a.upload(&setup.pc_a)?;

    common::write_test_file(&setup.pc_b, "conflict.txt", "version B")?;
    ops_b.save(&setup.pc_b, "B's version")?;

    // B에서 pull -> 경합
    ops_b.pull(&setup.pc_b)?;

    // B에서 Theirs를 선택하고 keep_other_copy=true
    let outcome = ops_b.resolve(
        &setup.pc_b,
        &[("conflict.txt".to_string(), Choice::Theirs)],
        true,
        "resolved: taking A's version with backup",
    )?;

    // 별명 파일이 생성되었는지 확인
    assert!(!outcome.copies.is_empty(), "should have backup copy");
    let backup_file = setup.pc_b.join(&outcome.copies[0]);
    assert!(backup_file.exists(), "backup file should exist");

    // 별명 파일의 내용이 B의 버전인지 확인
    let backup_content = std::fs::read_to_string(&backup_file)?;
    assert_eq!(backup_content, "version B");

    // 메인 파일의 내용이 A의 버전인지 확인
    let main_content = std::fs::read_to_string(setup.pc_b.join("conflict.txt"))?;
    assert_eq!(main_content, "version A");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_5_multiple_files_different_choices() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-5")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 두 파일을 초기화
    common::write_test_file(&setup.pc_a, "x.txt", "x original")?;
    common::write_test_file(&setup.pc_a, "y.txt", "y original")?;
    ops_a.save(&setup.pc_a, "initial")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;

    // A가 변경
    common::write_test_file(&setup.pc_a, "x.txt", "x from A")?;
    common::write_test_file(&setup.pc_a, "y.txt", "y from A")?;
    ops_a.save(&setup.pc_a, "A's changes")?;
    ops_a.upload(&setup.pc_a)?;

    // B가 다르게 변경
    common::write_test_file(&setup.pc_b, "x.txt", "x from B")?;
    common::write_test_file(&setup.pc_b, "y.txt", "y from B")?;
    ops_b.save(&setup.pc_b, "B's changes")?;

    // B에서 pull -> 경합
    ops_b.pull(&setup.pc_b)?;

    // x.txt는 Mine, y.txt는 Theirs 선택
    ops_b.resolve(
        &setup.pc_b,
        &[
            ("x.txt".to_string(), Choice::Mine),
            ("y.txt".to_string(), Choice::Theirs),
        ],
        false,
        "resolved with different choices",
    )?;

    // x.txt는 B의 버전
    let x_content = std::fs::read_to_string(setup.pc_b.join("x.txt"))?;
    assert_eq!(x_content, "x from B");

    // y.txt는 A의 버전
    let y_content = std::fs::read_to_string(setup.pc_b.join("y.txt"))?;
    assert_eq!(y_content, "y from A");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_6_deleted_by_them_keep_and_delete() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-6a")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 초기 파일
    common::write_test_file(&setup.pc_a, "del.txt", "content")?;
    ops_a.save(&setup.pc_a, "initial")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;

    // A에서 삭제
    std::fs::remove_file(setup.pc_a.join("del.txt"))?;
    ops_a.save(&setup.pc_a, "delete file")?;
    ops_a.upload(&setup.pc_a)?;

    // B에서 수정
    common::write_test_file(&setup.pc_b, "del.txt", "modified")?;
    ops_b.save(&setup.pc_b, "modify file")?;

    // B에서 pull -> 경합 (DeletedByThem)
    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::Conflicted { files } => {
            assert!(files
                .iter()
                .any(|f| f.path == "del.txt" && f.kind == core_ops::ConflictKind::DeletedByThem));
        }
        _ => panic!("expected Conflicted"),
    }

    // 테스트 6a: Mine (남기기)
    ops_b.resolve(
        &setup.pc_b,
        &[("del.txt".to_string(), Choice::Mine)],
        false,
        "keep the file",
    )?;

    assert!(setup.pc_b.join("del.txt").exists(), "file should be kept");
    let content = std::fs::read_to_string(setup.pc_b.join("del.txt"))?;
    assert_eq!(content, "modified");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_6b_deleted_by_them_accept_deletion() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-6b")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 초기 파일
    common::write_test_file(&setup.pc_a, "del.txt", "content")?;
    ops_a.save(&setup.pc_a, "initial")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;

    // A에서 삭제
    std::fs::remove_file(setup.pc_a.join("del.txt"))?;
    ops_a.save(&setup.pc_a, "delete file")?;
    ops_a.upload(&setup.pc_a)?;

    // B에서 수정
    common::write_test_file(&setup.pc_b, "del.txt", "modified")?;
    ops_b.save(&setup.pc_b, "modify file")?;

    // B에서 pull -> 경합
    ops_b.pull(&setup.pc_b)?;

    // 테스트 6b: Theirs (삭제 받아들이기)
    ops_b.resolve(
        &setup.pc_b,
        &[("del.txt".to_string(), Choice::Theirs)],
        false,
        "accept deletion",
    )?;

    assert!(
        !setup.pc_b.join("del.txt").exists(),
        "file should be deleted"
    );

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_7_abort_merge() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-7")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 경합을 일으킴
    common::write_test_file(&setup.pc_a, "conflict.txt", "original")?;
    ops_a.save(&setup.pc_a, "initial")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;

    common::write_test_file(&setup.pc_a, "conflict.txt", "A's version")?;
    ops_a.save(&setup.pc_a, "A's change")?;
    ops_a.upload(&setup.pc_a)?;

    common::write_test_file(&setup.pc_b, "conflict.txt", "B's version")?;
    ops_b.save(&setup.pc_b, "B's change")?;

    // pull -> 경합
    ops_b.pull(&setup.pc_b)?;

    // merge HEAD가 있는지 확인
    let (code, _, _) = common::run_git(&setup.pc_b, &["rev-parse", "MERGE_HEAD"]);
    assert_eq!(code, 0, "MERGE_HEAD should exist");

    // abort_merge
    ops_b.abort_merge(&setup.pc_b)?;

    // merge HEAD가 없는지 확인
    let (code, _, _) = common::run_git(&setup.pc_b, &["rev-parse", "MERGE_HEAD"]);
    assert_ne!(code, 0, "MERGE_HEAD should be gone");

    // 경합이 없는지 확인
    let conflicts = ops_b.conflicts(&setup.pc_b)?;
    assert!(conflicts.is_empty(), "conflicts should be gone");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_8_restore_points() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-8")?;

    let ops = Ops::new(new_runner());

    // save하면 restore point가 생성되는지 확인
    common::write_test_file(&setup.pc_a, "test.txt", "content")?;
    let outcome = ops.save(&setup.pc_a, "test save")?;

    match outcome {
        core_ops::SaveOutcome::Saved { restore_point, .. } => {
            // backup_ref가 생성되었는지 확인
            assert!(
                restore_point.backup_ref.is_some(),
                "backup_ref should be created"
            );
        }
        _ => panic!("expected Saved"),
    }

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_9_no_hikae_refs_on_remote() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-9")?;

    let ops = Ops::new(new_runner());

    // 여러 작업을 수행
    common::write_test_file(&setup.pc_a, "test.txt", "content")?;
    ops.save(&setup.pc_a, "save")?;
    ops.upload(&setup.pc_a)?;

    // bare repo에 refs/hikae/가 없는지 확인
    assert!(
        common::has_no_hikae_refs_on_remote(&setup.bare_repo),
        "refs/hikae/ should not exist on remote"
    );

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_10_no_global_config_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-10")?;

    let ops = Ops::new(new_runner());

    // 작업 수행
    common::write_test_file(&setup.pc_a, "test.txt", "content")?;
    ops.save(&setup.pc_a, "test")?;

    // PC-A에 local config가 설정되었는지 확인
    let (code, stdout, _) = common::run_git(&setup.pc_a, &["config", "--local", "core.autocrlf"]);
    assert_eq!(code, 0, "local config should be set");
    assert_eq!(stdout.trim(), "false");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_11_japanese_filename() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-11")?;

    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 일본어 파일명 생성
    common::write_test_file(&setup.pc_a, "メモ.txt", "content A")?;
    ops_a.save(&setup.pc_a, "add japanese file")?;
    ops_a.upload(&setup.pc_a)?;

    // B에서 pull
    ops_b.pull(&setup.pc_b)?;

    // B의 작업 폴더에 파일이 있는지 확인
    let memo_path = setup.pc_b.join("メモ.txt");
    assert!(memo_path.exists(), "japanese filename should work");

    let content = std::fs::read_to_string(&memo_path)?;
    assert_eq!(content, "content A");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_12_history_is_preserved_and_pre_merge_backup_points_to_old_head(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-12")?;
    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    // 共通の元になるファイルを両方に行き渡らせる
    common::write_test_file(&setup.pc_a, "doc.txt", "base\n")?;
    ops_a.save(&setup.pc_a, "base")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;
    let base_commit = get_head_commit(&setup.pc_a).ok_or("no HEAD in A")?;

    // A と B が同じ行を別内容に変更。A が先にアップロード
    common::write_test_file(&setup.pc_a, "doc.txt", "from A\n")?;
    ops_a.save(&setup.pc_a, "A edit")?;
    ops_a.upload(&setup.pc_a)?;
    let a_commit = get_head_commit(&setup.pc_a).ok_or("no HEAD in A")?;

    common::write_test_file(&setup.pc_b, "doc.txt", "from B\n")?;
    ops_b.save(&setup.pc_b, "B edit")?;
    let b_commit = get_head_commit(&setup.pc_b).ok_or("no HEAD in B")?;

    // 競合する取り込み。merge 前の HEAD が pre-merge バックアップ ref に残る
    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::Conflicted { .. } => {}
        other => panic!("expected Conflicted, got {other:?}"),
    }
    let (code, refs, _) = run_git(
        &setup.pc_b,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/hikae/backup/pre-merge/",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(
        refs.trim(),
        b_commit,
        "pre-merge backup ref should point to B's HEAD before merging"
    );

    // 解消（この PC の版）→ アップロードは通常の push で成功する
    ops_b.resolve(
        &setup.pc_b,
        &[("doc.txt".to_string(), Choice::Mine)],
        false,
        "merged",
    )?;
    match ops_b.upload(&setup.pc_b)? {
        core_ops::UploadOutcome::Pushed => {}
        other => panic!("expected Pushed, got {other:?}"),
    }

    // 公開側の履歴は書き換えられていない: 元のコミットも A/B のコミットも祖先として残る
    let (_, tip, _) = run_git(&setup.bare_repo, &["rev-parse", "main"]);
    let tip = tip.trim().to_string();
    for (name, c) in [("base", &base_commit), ("A", &a_commit), ("B", &b_commit)] {
        assert!(
            is_ancestor(&setup.bare_repo, c, &tip),
            "{name} commit must remain in the public history"
        );
    }
    // 先端は 2 親の merge commit
    let (_, parents, _) = run_git(
        &setup.bare_repo,
        &["rev-list", "--parents", "-n", "1", "main"],
    );
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "tip should be a merge commit"
    );
    assert!(has_no_hikae_refs_on_remote(&setup.bare_repo));

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_13_labels_are_injected_and_pull_creates_a_single_restore_point(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-13")?;
    let labels = core_ops::Labels {
        auto_save_memo: "memo-from-caller".to_string(),
        copy_mine: "mine-label".to_string(),
        copy_theirs: "theirs-label".to_string(),
    };
    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner()).with_labels(labels);

    // B に未保存の変更がある状態で、A の別ファイルの変更を取り込む
    common::write_test_file(&setup.pc_a, "from-a.txt", "a\n")?;
    ops_a.save(&setup.pc_a, "A change")?;
    ops_a.upload(&setup.pc_a)?;
    common::write_test_file(&setup.pc_b, "unsaved-b.txt", "b\n")?;

    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::Merged { .. } => {}
        other => panic!("expected Merged, got {other:?}"),
    }

    // 自動保存のメモは呼び出し側が渡した文言
    let (_, subjects, _) = run_git(&setup.pc_b, &["log", "--format=%s", "--no-merges"]);
    assert!(
        subjects.lines().any(|l| l == "memo-from-caller"),
        "auto-save memo should come from Labels: {subjects}"
    );
    // pull の復元点（バックアップ ref）は 1 回分だけ
    let (_, backups, _) = run_git(&setup.pc_b, &["for-each-ref", "refs/hikae/backup/pull/"]);
    assert_eq!(
        backups.lines().count(),
        1,
        "one restore point per pull: {backups}"
    );

    // 競合 → 使わなかった版の別名コピーにラベルが付き、元のフォルダに作られる
    common::write_test_file(&setup.pc_a, "dir/doc.txt", "base\n")?;
    ops_a.save(&setup.pc_a, "add doc")?;
    ops_a.upload(&setup.pc_a)?;
    ops_b.pull(&setup.pc_b)?;
    common::write_test_file(&setup.pc_a, "dir/doc.txt", "A\n")?;
    ops_a.save(&setup.pc_a, "A doc")?;
    ops_a.upload(&setup.pc_a)?;
    common::write_test_file(&setup.pc_b, "dir/doc.txt", "B\n")?;
    ops_b.save(&setup.pc_b, "B doc")?;
    ops_b.pull(&setup.pc_b)?;
    let outcome = ops_b.resolve(
        &setup.pc_b,
        &[("dir/doc.txt".to_string(), Choice::Mine)],
        true,
        "merged",
    )?;
    assert_eq!(outcome.copies.len(), 1);
    assert!(
        outcome.copies[0].starts_with("dir/doc (theirs-label ")
            && outcome.copies[0].ends_with(").txt"),
        "unexpected copy name: {}",
        outcome.copies[0]
    );
    assert_eq!(
        std::fs::read_to_string(setup.pc_b.join(&outcome.copies[0]))?,
        "A\n"
    );

    setup.cleanup()?;
    Ok(())
}
