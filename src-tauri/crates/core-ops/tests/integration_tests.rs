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

#[test]
fn test_14_list_files_at_handles_nonascii_and_spaces() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-14")?;
    let ops = Ops::new(new_runner());

    common::write_test_file(&setup.pc_a, "資料/報告 書.txt", "12345")?;
    common::write_test_file(&setup.pc_a, "dir/sub/a.png", "xy")?;
    let first = match ops.save(&setup.pc_a, "add files")? {
        core_ops::SaveOutcome::Saved { commit, .. } => commit,
        other => panic!("expected Saved, got {other:?}"),
    };
    // 後から別の変更を加えても、指定した時点の一覧は変わらない
    common::write_test_file(&setup.pc_a, "later.txt", "z")?;
    ops.save(&setup.pc_a, "later")?;

    let files = ops.list_files_at(&setup.pc_a, &first)?;
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["dir/sub/a.png", "file.txt", "資料/報告 書.txt"],
        "paths must be raw (no quoting) and limited to that commit"
    );
    let doc = files
        .iter()
        .find(|f| f.path == "資料/報告 書.txt")
        .ok_or("missing file")?;
    assert_eq!(doc.size, 5);
    assert_eq!(doc.mode, "100644");

    // オプションに見える値は拒否される
    assert!(ops.list_files_at(&setup.pc_a, "--help").is_err());
    assert!(ops.list_files_at(&setup.pc_a, "").is_err());

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_15_suggest_memo_from_real_worktree() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-15")?;
    let ops = Ops::new(new_runner());
    let labels = core_ops::MemoLabels::default();

    // 変更なし
    assert_eq!(ops.suggest_memo(&setup.pc_a, &labels)?, "");

    // 1 ファイル更新
    common::write_test_file(&setup.pc_a, "file.txt", "changed content")?;
    assert_eq!(ops.suggest_memo(&setup.pc_a, &labels)?, "Updated file.txt");
    ops.save(&setup.pc_a, "m")?;

    // 未追跡フォルダ内の画像 3 件は、フォルダにまとめず 3 件と数える
    for n in ["a.png", "b.jpg", "c.gif"] {
        common::write_test_file(&setup.pc_a, &format!("pics/{n}"), "img")?;
    }
    assert_eq!(ops.suggest_memo(&setup.pc_a, &labels)?, "Added 3 images");
    ops.save(&setup.pc_a, "pics")?;

    // 削除 1 件
    std::fs::remove_file(setup.pc_a.join("pics/a.png"))?;
    assert_eq!(ops.suggest_memo(&setup.pc_a, &labels)?, "Deleted a.png");
    ops.save(&setup.pc_a, "del")?;

    // 混在: 最も大きいファイル名 + 残り件数
    common::write_test_file(&setup.pc_a, "big.txt", &"x".repeat(5000))?;
    common::write_test_file(&setup.pc_a, "pics/b.jpg", "changed-image")?;
    assert_eq!(
        ops.suggest_memo(&setup.pc_a, &labels)?,
        "big.txt and 1 more"
    );

    // メモ生成は作業フォルダとインデックスを変えない（未追跡は未追跡のまま）
    let (_, status, _) = run_git(&setup.pc_a, &["status", "--porcelain"]);
    assert!(status.contains("?? big.txt"), "status: {status}");
    assert!(status.contains(" M pics/b.jpg"), "status: {status}");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_16_new_restore_points_identifies_refs_made_by_the_operation(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-16")?;
    let ops = Ops::new(new_runner());

    let before = ops.restore_point_refs(&setup.pc_a)?;
    common::write_test_file(&setup.pc_a, "new.txt", "n")?;
    let outcome = ops.save(&setup.pc_a, "save")?;
    let after = ops.restore_point_refs(&setup.pc_a)?;

    let created = core_ops::new_restore_points(&before, &after);
    match outcome {
        core_ops::SaveOutcome::Saved { restore_point, .. } => {
            assert_eq!(created.backup_ref, restore_point.backup_ref);
            assert_eq!(created.snapshot_ref, restore_point.snapshot_ref);
            assert!(created.backup_ref.is_some() || created.snapshot_ref.is_some());
        }
        other => panic!("expected Saved, got {other:?}"),
    }
    // 何もしなければ新規の復元点は無い
    let none = core_ops::new_restore_points(&after, &after);
    assert!(none.backup_ref.is_none() && none.snapshot_ref.is_none());

    setup.cleanup()?;
    Ok(())
}

/// pull の復元点（refs/hikae/backup/pull/*）の件数
fn count_pull_restore_points(repo: &std::path::Path) -> usize {
    let (_, out, _) = run_git(repo, &["for-each-ref", "refs/hikae/backup/pull/"]);
    out.lines().count()
}

#[test]
fn test_17_pull_without_updates_creates_no_restore_point() -> Result<(), Box<dyn std::error::Error>>
{
    let setup = TestSetup::new("test-17")?;
    let ops_b = Ops::new(new_runner());
    let head_before = get_head_commit(&setup.pc_b);

    // 更新も未保存の変更も無い取り込みは、何度繰り返しても復元点を増やさない
    for _ in 0..2 {
        match ops_b.pull(&setup.pc_b)? {
            core_ops::PullOutcome::UpToDate => {}
            other => panic!("expected UpToDate, got {other:?}"),
        }
    }
    assert_eq!(count_pull_restore_points(&setup.pc_b), 0);
    assert_eq!(get_head_commit(&setup.pc_b), head_before);

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_18_pull_with_updates_creates_one_restore_point() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-18")?;
    let ops_a = Ops::new(new_runner());
    let ops_b = Ops::new(new_runner());

    common::write_test_file(&setup.pc_a, "from-a.txt", "a\n")?;
    ops_a.save(&setup.pc_a, "A change")?;
    ops_a.upload(&setup.pc_a)?;

    match ops_b.pull(&setup.pc_b)? {
        core_ops::PullOutcome::FastForwarded => {}
        other => panic!("expected FastForwarded, got {other:?}"),
    }
    assert_eq!(count_pull_restore_points(&setup.pc_b), 1);
    assert!(setup.pc_b.join("from-a.txt").exists());

    // 取り込み後にもう一度呼んでも増えない
    ops_b.pull(&setup.pc_b)?;
    assert_eq!(count_pull_restore_points(&setup.pc_b), 1);

    // 未保存の変更だけがある場合は自動保存が走るため、復元点を作る
    common::write_test_file(&setup.pc_b, "unsaved.txt", "u\n")?;
    ops_b.pull(&setup.pc_b)?;
    let (_, all, _) = run_git(&setup.pc_b, &["for-each-ref", "refs/hikae/"]);
    assert!(
        !all.trim().is_empty(),
        "restore point expected before auto-save"
    );

    setup.cleanup()?;
    Ok(())
}

/// 保存して、その commit を返す
fn save_commit(
    ops: &Ops,
    repo: &std::path::Path,
    memo: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    match ops.save(repo, memo)? {
        core_ops::SaveOutcome::Saved { commit, .. } => Ok(commit),
        other => Err(format!("expected Saved, got {other:?}").into()),
    }
}

fn read(repo: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| format!("<{e}>"))
}

#[test]
fn test_19_restore_file_touches_only_that_file_and_can_be_undone(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-19")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "a.txt", "a1")?;
    common::write_test_file(repo, "b.txt", "b1")?;
    let c1 = save_commit(&ops, repo, "c1")?;
    common::write_test_file(repo, "a.txt", "a2")?;
    common::write_test_file(repo, "b.txt", "b2")?;
    save_commit(&ops, repo, "c2")?;
    // 未保存の変更と未追跡ファイル
    common::write_test_file(repo, "a.txt", "a3-unsaved")?;
    common::write_test_file(repo, "untracked.txt", "keep me")?;
    let head_before = get_head_commit(repo);

    // プレビュー
    let preview = ops.restore_file_preview(repo, &c1, "a.txt")?;
    assert_eq!(preview.kind, core_ops::RestoreFileKind::Overwrite);
    assert_eq!(preview.size_then, Some(2));

    let refs_before = ops.restore_point_refs(repo)?;
    let outcome = ops.restore_file(repo, &c1, "a.txt")?;
    let core_ops::RestoreFileOutcome::Restored { undo_ref } = outcome else {
        panic!("expected Restored, got {outcome:?}");
    };

    // 対象だけが戻り、他は変わらない
    assert_eq!(read(repo, "a.txt"), "a1");
    assert_eq!(read(repo, "b.txt"), "b2");
    assert_eq!(read(repo, "file.txt"), "initial content");
    assert_eq!(read(repo, "untracked.txt"), "keep me");
    assert_eq!(get_head_commit(repo), head_before, "履歴は変わらない");

    // 復元点が作られ、取り消し先がその 1 つを指す
    let refs_after = ops.restore_point_refs(repo)?;
    assert!(refs_after.len() > refs_before.len());
    let undo_ref = undo_ref.ok_or("undo_ref expected")?;
    assert!(undo_ref.starts_with("refs/hikae/"));
    assert!(refs_after.contains(&undo_ref));

    // 戻した後に作った未追跡ファイルは、取り消しでも消えない
    common::write_test_file(repo, "late.txt", "created after restore")?;
    // 取り消すと、戻す直前の未保存の内容に戻る。未追跡ファイルは残る
    ops.undo_restore(repo, &undo_ref)?;
    assert_eq!(read(repo, "a.txt"), "a3-unsaved");
    assert_eq!(read(repo, "b.txt"), "b2");
    assert_eq!(read(repo, "late.txt"), "created after restore");
    assert_eq!(read(repo, "untracked.txt"), "keep me");
    assert!(ops.restore_point_refs(repo)?.len() > refs_after.len());

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_20_restore_file_missing_at_that_point_deletes_nothing(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-20")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    let base = get_head_commit(repo).ok_or("no head")?;
    common::write_test_file(repo, "later.txt", "later")?;
    save_commit(&ops, repo, "add later")?;
    common::write_test_file(repo, "later.txt", "later-edited")?;

    let refs_before = ops.restore_point_refs(repo)?;
    let preview = ops.restore_file_preview(repo, &base, "later.txt")?;
    assert_eq!(preview.kind, core_ops::RestoreFileKind::NotInThatPoint);
    assert_eq!(
        ops.restore_file(repo, &base, "later.txt")?,
        core_ops::RestoreFileOutcome::NotInThatPoint
    );
    assert_eq!(
        read(repo, "later.txt"),
        "later-edited",
        "現在のファイルは消さない"
    );
    assert_eq!(
        ops.restore_point_refs(repo)?,
        refs_before,
        "何も変更しないので復元点も作らない"
    );

    // 消えているファイルは作り直せる
    std::fs::remove_file(repo.join("file.txt"))?;
    let preview = ops.restore_file_preview(repo, &base, "file.txt")?;
    assert_eq!(preview.kind, core_ops::RestoreFileKind::Recreate);
    ops.restore_file(repo, &base, "file.txt")?;
    assert_eq!(read(repo, "file.txt"), "initial content");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_21_restore_file_and_undo_reject_unsafe_input() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-21")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;
    let head = get_head_commit(repo).ok_or("no head")?;

    for bad in [
        "../outside.txt",
        "/etc/passwd",
        ".git/config",
        ":(top)file.txt",
        "",
    ] {
        assert!(ops.restore_file(repo, &head, bad).is_err(), "{bad:?}");
        assert!(
            ops.restore_file_preview(repo, &head, bad).is_err(),
            "{bad:?}"
        );
    }
    assert!(ops.restore_file(repo, "--hard", "file.txt").is_err());
    assert!(ops.restore_file(repo, "", "file.txt").is_err());

    // 復元点以外の ref・存在しない復元点・OID でないものは拒否する
    for bad in [
        "refs/heads/main",
        "main",
        "HEAD",
        "--hard",
        "refs/hikae/snapshots/main/not-there",
        "refs/hikae/../heads/main",
        head.get(..7).unwrap_or("abc"),
    ] {
        assert!(ops.undo_restore(repo, bad).is_err(), "{bad:?}");
    }
    // 復元点でない完全な OID（HEAD のコミット）も拒否する
    assert!(ops.undo_restore(repo, &head).is_err());

    // 復元点の完全な OID は受け付ける
    common::write_test_file(repo, "file.txt", "edited")?;
    ops.save(repo, "edit")?;
    let refs = ops.restore_point_refs(repo)?;
    let backup = refs
        .iter()
        .find(|r| r.starts_with("refs/hikae/backup/"))
        .ok_or("no backup")?;
    let (_, oid, _) = run_git(repo, &["rev-parse", backup]);
    ops.undo_restore(repo, oid.trim())?;

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_22_list_point_changes_reports_kinds() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-22")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    let long = "line of text that is long enough to be detected as a rename\n".repeat(20);
    common::write_test_file(repo, "old-name.txt", &long)?;
    common::write_test_file(repo, "gone.txt", "bye")?;
    common::write_test_file(repo, "edit.txt", "v1")?;
    save_commit(&ops, repo, "base")?;
    std::fs::rename(repo.join("old-name.txt"), repo.join("new-name.txt"))?;
    std::fs::remove_file(repo.join("gone.txt"))?;
    common::write_test_file(repo, "edit.txt", "v2")?;
    common::write_test_file(repo, "資料/追加 1.txt", "new")?;
    let c = save_commit(&ops, repo, "mix")?;

    let changes = ops.list_point_changes(repo, &c)?;
    let find = |p: &str| changes.iter().find(|c| c.path == p);
    assert_eq!(
        find("gone.txt").map(|c| c.kind),
        Some(core_ops::PointChangeKind::Deleted)
    );
    assert_eq!(
        find("edit.txt").map(|c| c.kind),
        Some(core_ops::PointChangeKind::Modified)
    );
    assert_eq!(
        find("資料/追加 1.txt").map(|c| c.kind),
        Some(core_ops::PointChangeKind::Added)
    );
    let renamed = find("new-name.txt").ok_or("rename missing")?;
    assert_eq!(renamed.kind, core_ops::PointChangeKind::Renamed);
    assert_eq!(renamed.old_path.as_deref(), Some("old-name.txt"));

    // 最初の保存は全ファイルが追加
    let (_, first, _) = run_git(repo, &["rev-list", "--max-parents=0", "HEAD"]);
    let root = ops.list_point_changes(repo, first.trim())?;
    assert!(root
        .iter()
        .all(|c| c.kind == core_ops::PointChangeKind::Added));
    assert!(root.iter().any(|c| c.path == "file.txt"));
    assert!(ops.list_point_changes(repo, "--help").is_err());

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_23_add_files_never_overwrites_and_rejects_bad_destinations(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-23")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;
    let src_dir = setup.pc_a.parent().ok_or("no parent")?.join("outside-src");
    std::fs::create_dir_all(&src_dir)?;
    let src = src_dir.join("x.txt");
    std::fs::write(&src, "from outside")?;
    std::fs::create_dir_all(repo.join("docs"))?;
    let head_before = get_head_commit(repo);

    // 直下へ追加。2回目は別名になり、1回目のファイルは上書きされない
    let first = ops.add_files(repo, std::slice::from_ref(&src), "")?;
    assert_eq!(first.added.len(), 1);
    assert_eq!(first.added[0].path, "x.txt");
    assert!(!first.added[0].renamed);
    std::fs::write(repo.join("x.txt"), "edited in project")?;
    let second = ops.add_files(repo, std::slice::from_ref(&src), "")?;
    assert_eq!(second.added[0].path, "x (2).txt");
    assert!(second.added[0].renamed);
    assert_eq!(read(repo, "x.txt"), "edited in project");
    assert_eq!(read(repo, "x (2).txt"), "from outside");
    assert!(src.exists(), "コピー元は動かさない");

    // サブフォルダへ
    let sub = ops.add_files(repo, std::slice::from_ref(&src), "docs")?;
    assert_eq!(sub.added[0].path, "docs/x.txt");
    assert_eq!(read(repo, "docs/x.txt"), "from outside");

    // 保存はしない（履歴は変わらず、未保存の変更として残る）
    assert_eq!(get_head_commit(repo), head_before);
    assert!(ops.sync_state(repo)?.dirty);

    // 復元点が作られる
    assert!(!ops.restore_point_refs(repo)?.is_empty());

    // 不正な追加先
    for bad in [
        "..",
        "../outside-src",
        "docs/../..",
        "/tmp",
        "C:\\Windows",
        ".git",
        ".git/hooks",
        "no-such-dir",
    ] {
        assert!(
            ops.add_files(repo, std::slice::from_ref(&src), bad)
                .is_err(),
            "{bad:?}"
        );
    }
    // 追加先がファイルの場合も拒否する
    assert!(ops
        .add_files(repo, std::slice::from_ref(&src), "x.txt")
        .is_err());

    // フォルダ・存在しないもの・100MB 超は追加せず理由を返す
    let big = src_dir.join("big.bin");
    let f = std::fs::File::create(&big)?;
    f.set_len(core_ops::LARGE_FILE_LIMIT_BYTES + 1)?;
    let warn = src_dir.join("warn.bin");
    let f = std::fs::File::create(&warn)?;
    f.set_len(core_ops::LARGE_FILE_WARN_BYTES)?;
    // 警告の境界は「超」（サイズ検査と同じ）。ちょうど 50MB は警告せず、1 バイトでも超えれば警告する
    let over = src_dir.join("over.bin");
    let f = std::fs::File::create(&over)?;
    f.set_len(core_ops::LARGE_FILE_WARN_BYTES + 1)?;
    let sources = vec![
        src_dir.clone(),
        src_dir.join("missing.txt"),
        big,
        warn,
        over,
    ];
    let out = ops.add_files(repo, &sources, "")?;
    assert_eq!(out.added.len(), 2);
    assert_eq!(out.added[0].path, "warn.bin");
    assert!(!out.added[0].large, "ちょうど 50MB は警告しない");
    assert_eq!(out.added[1].path, "over.bin");
    assert!(out.added[1].large, "50MB を 1 バイトでも超えれば警告する");
    assert_eq!(out.rejected.len(), 3);
    assert!(out.rejected.iter().any(|r| matches!(
        r.reason,
        core_ops::AddRejectReason::TooLarge { size } if size == core_ops::LARGE_FILE_LIMIT_BYTES + 1
    )));
    assert!(out
        .rejected
        .iter()
        .any(|r| r.reason == core_ops::AddRejectReason::NotAFile));
    assert!(out
        .rejected
        .iter()
        .any(|r| r.reason == core_ops::AddRejectReason::Unreadable));
    assert!(!repo.join("big.bin").exists());

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_24_check_relocation_requires_same_repository() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-24")?;
    let ops = Ops::new(new_runner());
    let url = setup.bare_repo.to_string_lossy().to_string();
    let identity = core_ops::Identity {
        name: "x".to_string(),
        email: "x@users.noreply.github.com".to_string(),
    };

    // 同じ保存先を持つ別の場所（PC-B）は同一とみなす
    assert_eq!(
        ops.check_relocation(&setup.pc_b, Some(&url))?,
        core_ops::RelocateCheck::Same
    );

    // 別のリポジトリ（保存先が違う）は拒否
    let parent = setup.pc_a.parent().ok_or("no parent")?;
    let other = parent.join("other-project");
    ops.init_project(
        &other,
        Some("https://example.com/someone/else.git"),
        &identity,
    )?;
    assert_eq!(
        ops.check_relocation(&other, Some(&url))?,
        core_ops::RelocateCheck::DifferentRepository
    );

    // 保存先が無いリポジトリも別物として拒否
    let no_remote = parent.join("no-remote");
    ops.init_project(&no_remote, None, &identity)?;
    assert_eq!(
        ops.check_relocation(&no_remote, Some(&url))?,
        core_ops::RelocateCheck::DifferentRepository
    );

    // 照合できる登録情報が無ければ確認不能
    assert_eq!(
        ops.check_relocation(&setup.pc_b, None)?,
        core_ops::RelocateCheck::CannotVerify
    );

    // リポジトリの最上位でない（サブフォルダ）・存在しない
    std::fs::create_dir_all(setup.pc_b.join("sub"))?;
    assert_eq!(
        ops.check_relocation(&setup.pc_b.join("sub"), Some(&url))?,
        core_ops::RelocateCheck::NotARepository
    );
    assert_eq!(
        ops.check_relocation(&setup.pc_b.join("nope"), Some(&url))?,
        core_ops::RelocateCheck::NotARepository
    );

    // 確認だけでファイルは変わらない
    assert_eq!(read(&setup.pc_b, "file.txt"), "initial content");

    setup.cleanup()?;
    Ok(())
}

/// 履歴のうち、手動の保存だけを取り出す（自動保存は `snapshot_ref` を持つ）
fn manual_entries(entries: &[core_ops::HistoryEntry]) -> Vec<&core_ops::HistoryEntry> {
    entries
        .iter()
        .filter(|e| e.snapshot_ref.is_none())
        .collect()
}

#[test]
fn test_25_history_lists_multiple_saves_with_file_counts() -> Result<(), Box<dyn std::error::Error>>
{
    let setup = TestSetup::new("test-25")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "a.txt", "a1")?;
    common::write_test_file(repo, "b.txt", "b1")?;
    let c1 = save_commit(&ops, repo, "first")?;
    common::write_test_file(repo, "a.txt", "a2")?;
    let c2 = save_commit(&ops, repo, "second")?;
    std::fs::remove_file(repo.join("b.txt"))?;
    common::write_test_file(repo, "c.txt", "c1")?;
    let c3 = save_commit(&ops, repo, "third")?;

    let entries = ops.history(repo, 50)?;
    // 保存の直前に作られる復元点（スナップショット）は、直後の保存と同じ内容なので出さない
    assert!(
        entries.iter().all(|e| e.snapshot_ref.is_none()),
        "手動の保存だけが並ぶ: {entries:?}"
    );
    let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, ["third", "second", "first", "initial commit"]);
    assert!(c3.starts_with(&entries[0].commit));
    assert!(c2.starts_with(&entries[1].commit));
    assert!(c1.starts_with(&entries[2].commit));
    let counts: Vec<u32> = entries.iter().map(|e| e.changed_files_count).collect();
    // third は b.txt の削除と c.txt の追加、initial commit は file.txt の追加
    assert_eq!(counts, [2, 1, 2, 1]);

    // 件数の上限が効く
    assert_eq!(ops.history(repo, 2)?.len(), 2);

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_26_history_keeps_memo_with_pipe_and_newline() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-26")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    let memo = "見積 | 請求 | 納品\n2行目 | 補足\n\n3段落目";
    common::write_test_file(repo, "a.txt", "a")?;
    save_commit(&ops, repo, memo)?;
    common::write_test_file(repo, "a.txt", "b")?;
    save_commit(&ops, repo, "||")?;

    let entries = ops.history(repo, 10)?;
    assert_eq!(entries[0].message, "||");
    assert_eq!(
        entries[1].message, memo,
        "メモは区切り文字や改行を含んでも欠けない"
    );
    assert_eq!(entries[0].changed_files_count, 1);
    assert_eq!(entries.len(), 3);

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_27_history_handles_root_commit() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new().prefix("test-27").tempdir()?;
    let repo = tmp.path().join("fresh");
    let ops = Ops::new(new_runner());
    ops.init_project(
        &repo,
        None,
        &core_ops::Identity {
            name: "x".to_string(),
            email: "x@users.noreply.github.com".to_string(),
        },
    )?;
    // まだ保存が無い（HEAD が無い）プロジェクトは空の履歴
    assert!(ops.history(&repo, 10)?.is_empty());

    common::write_test_file(&repo, "a.txt", "a")?;
    common::write_test_file(&repo, "dir/b.txt", "b")?;
    common::write_test_file(&repo, "dir/c.txt", "c")?;
    save_commit(&ops, &repo, "root")?;

    let entries = ops.history(&repo, 10)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "root");
    assert_eq!(
        entries[0].changed_files_count, 3,
        "ルートは全ファイルが追加扱い"
    );
    assert!(entries[0].snapshot_ref.is_none());
    Ok(())
}

#[test]
fn test_28_history_marks_snapshots_as_auto_saves() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-28")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "a.txt", "a1")?;
    let c1 = save_commit(&ops, repo, "c1")?;
    common::write_test_file(repo, "a.txt", "a2")?;
    save_commit(&ops, repo, "c2")?;

    // 未保存の変更がある状態で 1 ファイルを戻す → 復元点として自動保存が作られる
    common::write_test_file(repo, "a.txt", "a3-unsaved")?;
    common::write_test_file(repo, "new.txt", "unsaved new file")?;
    ops.restore_file(repo, &c1, "a.txt")?;

    let entries = ops.history(repo, 50)?;
    let autos: Vec<_> = entries
        .iter()
        .filter(|e| e.snapshot_ref.is_some())
        .collect();
    assert_eq!(autos.len(), 1, "自動保存は 1 件だけ: {entries:?}");
    let auto = autos[0];
    let snapshot_ref = auto.snapshot_ref.as_deref().ok_or("snapshot_ref")?;
    assert!(
        snapshot_ref.starts_with("refs/hikae/snapshots/"),
        "{snapshot_ref}"
    );
    assert!(auto.message.starts_with("auto:"), "{}", auto.message);
    assert_eq!(auto.changed_files_count, 2, "a.txt の更新と new.txt の追加");

    // 手動の保存は区別されたまま、復元点（backup）は履歴に出ない
    let manual = manual_entries(&entries);
    let messages: Vec<&str> = manual.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, ["c2", "c1", "initial commit"]);
    assert!(entries.iter().all(|e| !e.message.starts_with("backup")));

    // 自動保存の commit は公開側の履歴（HEAD）には入っていない
    let (_, head_log, _) = run_git(repo, &["log", "--format=%s", "HEAD"]);
    assert!(!head_log.contains("auto:"), "{head_log}");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_29_history_counts_japanese_file_names() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-29")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "資料/第3章 報告書.txt", "本文")?;
    common::write_test_file(repo, "資料/図表①.txt", "図")?;
    save_commit(&ops, repo, "日本語のファイルを追加")?;
    common::write_test_file(repo, "資料/第3章 報告書.txt", "本文を更新")?;
    save_commit(&ops, repo, "更新")?;

    let entries = ops.history(repo, 10)?;
    assert_eq!(entries[0].message, "更新");
    assert_eq!(entries[0].changed_files_count, 1);
    assert_eq!(entries[1].message, "日本語のファイルを追加");
    assert_eq!(entries[1].changed_files_count, 2);

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_30_restore_returns_a_token_that_undoes_it() -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-30")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "a.txt", "a1")?;
    let c1 = save_commit(&ops, repo, "c1")?;
    common::write_test_file(repo, "a.txt", "a2")?;
    common::write_test_file(repo, "b.txt", "b2")?;
    save_commit(&ops, repo, "c2")?;

    // 未保存の変更がある状態で全体を戻す → 取り消し先は戻す直前の作業状態（スナップショット）
    common::write_test_file(repo, "a.txt", "a3-unsaved")?;
    common::write_test_file(repo, "untracked.txt", "keep me")?;
    let refs_before = ops.restore_point_refs(repo)?;
    let token = ops
        .restore(repo, &c1)?
        .ok_or("restore must return an undo token")?;
    assert!(token.starts_with("refs/hikae/snapshots/"), "{token}");
    assert!(ops.restore_point_refs(repo)?.contains(&token));
    assert!(ops.restore_point_refs(repo)?.len() > refs_before.len());
    assert_eq!(read(repo, "a.txt"), "a1");
    assert!(
        !repo.join("b.txt").exists(),
        "その時点に無い追跡ファイルは戻る"
    );
    assert_eq!(read(repo, "untracked.txt"), "keep me");

    ops.undo_restore(repo, &token)?;
    assert_eq!(read(repo, "a.txt"), "a3-unsaved");
    assert_eq!(read(repo, "b.txt"), "b2");
    assert_eq!(read(repo, "untracked.txt"), "keep me");

    // 未保存の変更が無い状態では、取り消し先は HEAD の控え（backup）
    save_commit(&ops, repo, "c3")?;
    let token = ops
        .restore(repo, &c1)?
        .ok_or("restore must return an undo token")?;
    assert!(token.starts_with("refs/hikae/backup/restore/"), "{token}");
    assert_eq!(read(repo, "a.txt"), "a1");
    ops.undo_restore(repo, &token)?;
    assert_eq!(read(repo, "a.txt"), "a3-unsaved");
    assert_eq!(read(repo, "b.txt"), "b2");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_31_relocation_without_remote_uses_initial_commit() -> Result<(), Box<dyn std::error::Error>>
{
    let setup = TestSetup::new("test-31")?;
    let ops = Ops::new(new_runner());
    let identity = core_ops::Identity {
        name: "x".to_string(),
        email: "x@users.noreply.github.com".to_string(),
    };
    let parent = setup.pc_a.parent().ok_or("no parent")?;

    // 保存先の無いプロジェクト
    let project = parent.join("local-only");
    ops.init_project(&project, None, &identity)?;
    assert_eq!(ops.initial_commit(&project)?, None, "保存前は記録できない");
    common::write_test_file(&project, "a.txt", "a")?;
    let first = save_commit(&ops, &project, "first")?;
    common::write_test_file(&project, "a.txt", "b")?;
    save_commit(&ops, &project, "second")?;
    let initial = ops
        .initial_commit(&project)?
        .ok_or("initial commit expected")?;
    assert_eq!(initial, first, "最初の保存の OID");

    // フォルダを移動した想定（コピー）。履歴が同じなら同一とみなす
    let moved = parent.join("moved");
    let (code, _, err) = common::run_git(
        parent,
        &[
            "clone",
            &project.to_string_lossy(),
            &moved.to_string_lossy(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        ops.check_relocation_with(&moved, None, Some(&initial))?,
        core_ops::RelocateCheck::Same
    );

    // 別のプロジェクト（初期の保存が違う）は拒否
    let other = parent.join("other-local");
    ops.init_project(&other, None, &identity)?;
    common::write_test_file(&other, "a.txt", "a")?;
    save_commit(&ops, &other, "first")?;
    assert_eq!(
        ops.check_relocation_with(&other, None, Some(&initial))?,
        core_ops::RelocateCheck::DifferentRepository
    );

    // 保存が 1 つも無いフォルダも別物
    let empty = parent.join("empty-repo");
    ops.init_project(&empty, None, &identity)?;
    assert_eq!(
        ops.check_relocation_with(&empty, None, Some(&initial))?,
        core_ops::RelocateCheck::DifferentRepository
    );

    // 保存先も初期の保存も未登録なら、従来どおり確認不能
    assert_eq!(
        ops.check_relocation_with(&moved, None, None)?,
        core_ops::RelocateCheck::CannotVerify
    );
    assert_eq!(
        ops.check_relocation_with(&moved, Some(""), Some(""))?,
        core_ops::RelocateCheck::CannotVerify
    );

    // 保存先 URL がある場合は URL の照合が優先される（初期の保存が合っていても URL が違えば拒否）
    let url = setup.bare_repo.to_string_lossy().to_string();
    assert_eq!(
        ops.check_relocation_with(&moved, Some(&url), Some(&initial))?,
        core_ops::RelocateCheck::DifferentRepository
    );

    // 確認だけでファイルは変わらない
    assert_eq!(read(&moved, "a.txt"), "b");

    setup.cleanup()?;
    Ok(())
}

/// Windows で、他のアプリがファイルを排他で開いている状態を再現して E12 を確認する
#[cfg(windows)]
#[test]
fn test_32_file_held_open_by_another_app_is_reported_as_in_use(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::fs::OpenOptionsExt;

    let setup = TestSetup::new("test-32")?;
    let ops = Ops::new(new_runner());
    let repo = &setup.pc_a;

    common::write_test_file(repo, "第3章.docx", "v1")?;
    let c1 = save_commit(&ops, repo, "v1")?;
    common::write_test_file(repo, "第3章.docx", "v2")?;
    save_commit(&ops, repo, "v2")?;

    // 共有を許可しない（share_mode 0）で開いたままにする
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(repo.join("第3章.docx"))?;

    let result = ops.restore(repo, &c1);
    match &result {
        Err(core_ops::OpsError::FileInUse { .. }) => {}
        other => panic!("expected FileInUse, got {other:?}"),
    }
    drop(held);
    // 閉じれば同じ操作が成功する
    ops.restore(repo, &c1)?;
    assert_eq!(read(repo, "第3章.docx"), "v1");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn clone_refuses_a_non_empty_destination_and_accepts_an_empty_one(
) -> Result<(), Box<dyn std::error::Error>> {
    let setup = TestSetup::new("test-clone-dest")?;
    let ops = Ops::new(new_runner());
    let url = setup.bare_repo.to_string_lossy().to_string();
    let identity = core_ops::Identity {
        name: "pc-c".to_string(),
        email: "pc-c@users.noreply.github.com".to_string(),
    };
    let root = setup.pc_a.parent().ok_or("no parent")?.to_path_buf();

    // 空でないフォルダ: 拒否し、中身には触れない
    let busy = root.join("busy");
    std::fs::create_dir_all(&busy)?;
    std::fs::write(busy.join("mine.txt"), "keep")?;
    let result = ops.clone_project(&url, &busy, &identity);
    assert!(matches!(result, Err(core_ops::OpsError::InvalidInput(_))));
    assert_eq!(std::fs::read_to_string(busy.join("mine.txt"))?, "keep");
    assert!(!busy.join(".git").exists());

    // ファイルがある場所: 拒否
    let file_path = root.join("a-file");
    std::fs::write(&file_path, "x")?;
    assert!(matches!(
        ops.clone_project(&url, &file_path, &identity),
        Err(core_ops::OpsError::InvalidInput(_))
    ));

    // 空の既存フォルダ: 取得できる
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty)?;
    ops.clone_project(&url, &empty, &identity)?;
    assert_eq!(read(&empty, "file.txt"), "initial content");

    // 存在しない場所: 取得できる
    let fresh = root.join("fresh");
    ops.clone_project(&url, &fresh, &identity)?;
    assert_eq!(read(&fresh, "file.txt"), "initial content");

    setup.cleanup()?;
    Ok(())
}

#[test]
fn test_apply_identity_updates_local_config_and_commit_author(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::Builder::new().prefix("identity").tempdir()?;
    let repo = tmp.path().join("proj");
    let ops = Ops::new(new_runner());

    // 未ログイン想定の既定で作成する
    ops.init_project(&repo, None, &core_ops::resolve_identity(None))?;
    let (_, name, _) = common::run_git(&repo, &["config", "--local", "user.name"]);
    assert_eq!(name.trim(), core_ops::FALLBACK_NAME);

    // ログイン後の実ユーザーへ更新して保存する
    let real = core_ops::resolve_identity(Some((12345, "alice")));
    ops.apply_identity(&repo, &real)?;
    common::write_test_file(&repo, "a.txt", "hello")?;
    ops.save(&repo, "first")?;

    let (_, author, _) = common::run_git(&repo, &["log", "-1", "--format=%an <%ae>"]);
    assert_eq!(
        author.trim(),
        "alice <12345+alice@users.noreply.github.com>"
    );
    Ok(())
}
