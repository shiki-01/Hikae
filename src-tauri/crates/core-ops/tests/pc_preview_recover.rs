// PC 名のトレーラー、過去の版の書き出し、中断された操作の復旧、取り込み前の自動保存のサイズ検査、
// 競合解消後の書き戻し、復元点の署名の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 時間計測（sleep・経過時間のしきい値）は使わない。時刻は固定の clock で与える。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{
    cleanup_old_previews, Choice, Identity, Ops, OpsError, PullOutcome, SizeLimits, UploadOutcome,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use time::OffsetDateTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 復旧テストで使う固定の時刻（Unix 秒）
const FIXED_UNIX: i64 = 1_800_000_000;

const LIMITS: SizeLimits = SizeLimits {
    warn_bytes: 1_000,
    block_bytes: 5_000,
};

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

/// PC 名を指定した Ops（PC 名を指定しない環境依存を避ける）
fn ops_named(pc: Option<&str>) -> Ops {
    Ops::new(GitRunner::from_path_env()).with_pc_name(pc.map(str::to_string))
}

/// 時刻を固定した Ops
fn ops_fixed(pc: Option<&str>) -> Ops {
    ops_named(pc).with_clock(|| {
        OffsetDateTime::from_unix_timestamp(FIXED_UNIX).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    })
}

struct TwoPcs {
    _tmp: tempfile::TempDir,
    bare: PathBuf,
    pc_a: PathBuf,
    pc_b: PathBuf,
}

/// PC-A が 3 ファイルを保存してアップロードし、PC-B がそれを取得した状態
fn two_pcs() -> Result<TwoPcs, Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    let a = ops_named(Some("PC-A"));
    a.init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    std::fs::write(pc_a.join("keep.txt"), "keep v1\n")?;
    std::fs::write(pc_a.join("conflict.txt"), "line1\nline2\nline3\n")?;
    std::fs::write(pc_a.join("secret.bin"), [0u8, 255, 1, 2, 3, 10, 13])?;
    a.save(&pc_a, "initial")?;
    a.upload(&pc_a)?;
    ops_named(Some("PC-B")).clone_project(&url, &pc_b, &identity("pc-b"))?;
    Ok(TwoPcs {
        _tmp: tmp,
        bare,
        pc_a,
        pc_b,
    })
}

fn head_message(repo: &Path) -> String {
    run_git(repo, &["log", "-1", "--format=%B"]).1
}

fn refs(repo: &Path) -> String {
    run_git(repo, &["for-each-ref", "refs/hikae/"]).1
}

// ---------------------------------------------------------------- PC 名

#[test]
fn save_and_pull_append_the_pc_name_and_history_hides_it() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);

    // 保存: メモの末尾にトレーラーが付く。履歴のメモには含まれず、pc_name に分かれる
    std::fs::write(a.join("keep.txt"), "keep v2\n")?;
    ops_named(Some("PC-A")).save(a, "第2版に更新\n2行目")?;
    assert_eq!(
        head_message(a).trim_end(),
        "第2版に更新\n2行目\n\nHikae-PC: PC-A"
    );
    let entries = ops_named(Some("PC-A")).history(a, 10)?;
    assert_eq!(entries[0].message, "第2版に更新\n2行目");
    assert_eq!(entries[0].pc_name.as_deref(), Some("PC-A"));
    // トレーラーの無い古い保存は pc_name が None
    let last = entries.last().ok_or("no entries")?;
    assert_eq!(last.message, "initial");
    assert_eq!(last.pc_name.as_deref(), Some("PC-A"));

    // PC 名が取得できない環境ではトレーラーを付けない
    std::fs::write(a.join("keep.txt"), "keep v3\n")?;
    ops_named(None).save(a, "名前なし")?;
    assert_eq!(head_message(a).trim_end(), "名前なし");
    let entries = ops_named(None).history(a, 10)?;
    assert_eq!(entries[0].message, "名前なし");
    assert_eq!(entries[0].pc_name, None);

    // 取り込み前の自動保存にも付く
    ops_named(Some("PC-A")).upload(a)?;
    std::fs::write(b.join("draft.txt"), "draft\n")?;
    ops_named(Some("PC-B")).pull(b)?;
    let (_, log, _) = run_git(b, &["log", "--format=%B", "--max-count=5"]);
    assert!(log.contains("Hikae-PC: PC-B"), "{log}");
    Ok(())
}

#[test]
fn control_characters_in_the_pc_name_never_reach_the_commit_message() -> TestResult {
    let pcs = two_pcs()?;
    let a = &pcs.pc_a;
    std::fs::write(a.join("keep.txt"), "x\n")?;
    ops_named(Some("PC\nSigned-off-by: evil\r\n")).save(a, "memo")?;
    let msg = head_message(a);
    assert_eq!(
        msg.trim_end(),
        "memo\n\nHikae-PC: PCSigned-off-by: evil",
        "改行は除かれ、別の行を作れない"
    );
    Ok(())
}

#[test]
fn conflicts_report_the_pc_name_of_each_side() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    std::fs::write(a.join("conflict.txt"), "line1 from A\nline2\nline3\n")?;
    ops_named(Some("PC-A")).save(a, "A edit")?;
    ops_named(Some("PC-A")).upload(a)?;
    std::fs::write(b.join("conflict.txt"), "line1 from B\nline2\nline3\n")?;
    let ops_b = ops_named(Some("PC-B"));
    ops_b.save(b, "B edit")?;

    let PullOutcome::Conflicted { files } = ops_b.pull(b)? else {
        panic!("expected Conflicted");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].this_pc_name.as_deref(), Some("PC-B"));
    assert_eq!(files[0].cloud_pc_name.as_deref(), Some("PC-A"));
    let listed = ops_b.conflicts(b)?;
    assert_eq!(listed[0].this_pc_name.as_deref(), Some("PC-B"));
    assert_eq!(listed[0].cloud_pc_name.as_deref(), Some("PC-A"));

    // 解消の保存にも PC 名が付く
    ops_b.resolve(
        b,
        &[("conflict.txt".to_string(), Choice::Mine)],
        false,
        "まとめた",
    )?;
    assert_eq!(head_message(b).trim_end(), "まとめた\n\nHikae-PC: PC-B");
    Ok(())
}

// ---------------------------------------------------------------- 過去の版を開く

#[test]
fn export_file_at_writes_the_exact_bytes_read_only() -> TestResult {
    let pcs = two_pcs()?;
    let a = &pcs.pc_a;
    let ops = ops_named(Some("PC-A"));
    let first = get_head_commit(a).ok_or("no head")?;

    // 2 つ目の版: テキストを更新し、バイナリも変える。サブフォルダ・日本語・空白の名前も用意する
    std::fs::create_dir_all(a.join("資料"))?;
    let binary_v1: Vec<u8> = (0u8..=255).chain([0, 13, 10, 13, 10, 0]).collect();
    std::fs::write(a.join("資料/図 1.bin"), &binary_v1)?;
    ops.save(a, "add binary")?;
    let second = get_head_commit(a).ok_or("no head")?;
    std::fs::write(a.join("keep.txt"), "keep v2\r\n")?;
    std::fs::write(a.join("資料/図 1.bin"), [9u8, 8, 7])?;
    ops.save(a, "update")?;

    let root = tempfile::tempdir()?;
    let preview = root.path().join("proj-id");

    // バイナリ: 過去の版の内容が一致する（現在の内容ではない）
    let p = ops.export_file_at(a, &second[..7], "資料/図 1.bin", &preview)?;
    assert_eq!(std::fs::read(&p)?, binary_v1);
    assert!(p.starts_with(&preview));
    assert!(p.ends_with("資料/図 1.bin") || p.ends_with("資料\\図 1.bin"));
    assert!(std::fs::metadata(&p)?.permissions().readonly());
    // 作業フォルダの内容は変わらない
    assert_eq!(std::fs::read(a.join("資料/図 1.bin"))?, [9u8, 8, 7]);

    // テキスト（CRLF を含む）も、最初の版と現在で別の内容になる
    let v1 = ops.export_file_at(a, &first, "keep.txt", &preview)?;
    assert_eq!(std::fs::read(&v1)?, b"keep v1\n");
    let head = get_head_commit(a).ok_or("no head")?;
    let v2 = ops.export_file_at(a, &head, "keep.txt", &preview)?;
    assert_eq!(std::fs::read(&v2)?, b"keep v2\r\n");
    assert_ne!(v1, v2, "コミットごとに別の場所へ書く");

    // 同じ版をもう一度書き出しても成功し、内容は同じ（読み取り専用でも上書きで失敗しない）
    let again = ops.export_file_at(a, &first, "keep.txt", &preview)?;
    assert_eq!(again, v1);
    assert_eq!(std::fs::read(&again)?, b"keep v1\n");
    Ok(())
}

#[test]
fn export_file_at_rejects_unsafe_paths_and_missing_files() -> TestResult {
    let pcs = two_pcs()?;
    let a = &pcs.pc_a;
    let ops = ops_named(Some("PC-A"));
    let head = get_head_commit(a).ok_or("no head")?;
    let root = tempfile::tempdir()?;
    let preview = root.path().join("proj-id");
    let outside = root.path().join("escaped.txt");

    for bad in [
        "../escaped.txt",
        "a/../../escaped.txt",
        "..\\escaped.txt",
        "/etc/passwd",
        "C:\\Windows\\win.ini",
        "\\\\server\\share\\x",
        ".git/config",
        ".GIT/HEAD",
        "sub/.git/config",
        "",
        ":(top)keep.txt",
    ] {
        assert!(
            matches!(
                ops.export_file_at(a, &head, bad, &preview),
                Err(OpsError::InvalidInput(_))
            ),
            "{bad:?}"
        );
    }
    // フォルダ・存在しないファイル・不正なコミット指定
    assert!(ops
        .export_file_at(a, &head, "missing.txt", &preview)
        .is_err());
    assert!(ops.export_file_at(a, "-p", "keep.txt", &preview).is_err());
    assert!(ops
        .export_file_at(a, "no-such-commit", "keep.txt", &preview)
        .is_err());
    assert!(!outside.exists());
    assert!(!preview.exists(), "拒否した場合は何も書かない");
    Ok(())
}

#[test]
fn cleanup_removes_only_old_preview_folders() -> TestResult {
    let root = tempfile::tempdir()?;
    let old_file = root.path().join("proj-1/aaaaaaaaaaaa/docs/old.txt");
    let new_file = root.path().join("proj-1/bbbbbbbbbbbb/new.txt");
    let other_old = root.path().join("proj-2/cccccccccccc/old.txt");
    for f in [&old_file, &new_file, &other_old] {
        std::fs::create_dir_all(f.parent().ok_or("no parent")?)?;
        std::fs::write(f, "x")?;
    }
    let now = SystemTime::now();
    let ten_days_ago = now - Duration::from_secs(10 * 24 * 60 * 60);
    for f in [&old_file, &other_old] {
        let file = std::fs::OpenOptions::new().write(true).open(f)?;
        file.set_modified(ten_days_ago)?;
    }
    // 古いファイルは読み取り専用でも消せる
    let mut perm = std::fs::metadata(&old_file)?.permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&old_file, perm)?;
    // プロジェクトの外のファイルには触れない
    let unrelated = root.path().join("unrelated.txt");
    std::fs::write(&unrelated, "keep")?;

    let removed = cleanup_old_previews(root.path(), Duration::from_secs(7 * 24 * 60 * 60), now);
    assert_eq!(removed, 2);
    assert!(!root.path().join("proj-1/aaaaaaaaaaaa").exists());
    assert!(!root.path().join("proj-2").exists(), "空になったら片付ける");
    assert!(new_file.exists());
    assert!(unrelated.exists());
    // 無いフォルダは 0 件
    assert_eq!(
        cleanup_old_previews(&root.path().join("nope"), Duration::from_secs(1), now),
        0
    );
    Ok(())
}

// ---------------------------------------------------------------- 中断された操作の復旧

#[test]
fn recover_restores_the_working_folder_from_before_an_interrupted_pull() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    // PC-A が新しいファイルを足して keep.txt を更新する
    std::fs::write(a.join("a-new.txt"), "from A\n")?;
    std::fs::write(a.join("keep.txt"), "keep v2 (A)\n")?;
    ops_named(Some("PC-A")).save(a, "A change")?;
    ops_named(Some("PC-A")).upload(a)?;
    // PC-B には未保存の変更（追跡中の変更と未追跡の新規ファイル）がある
    std::fs::write(b.join("conflict.txt"), "edited on B, unsaved\n")?;
    std::fs::write(b.join("draft.txt"), "draft\n")?;

    // 取り込みは最後まで終わったが、その後の処理で止まった、という状態を作る
    // （ジャーナル上は「実行中」のまま残り、起動時に「中断」になる）
    let ops_b = ops_fixed(Some("PC-B"));
    assert!(matches!(ops_b.pull(b)?, PullOutcome::Merged { .. }));
    assert!(b.join("a-new.txt").exists());
    let head_after_pull = get_head_commit(b);

    let restored = ops_b.recover_interrupted(b, "pull", FIXED_UNIX)?;
    // 取り込み前の未保存の変更を含む自動保存へ戻る
    assert!(restored.starts_with("refs/hikae/snapshots/"), "{restored}");
    assert_eq!(
        std::fs::read_to_string(b.join("conflict.txt"))?,
        "edited on B, unsaved\n"
    );
    assert_eq!(std::fs::read_to_string(b.join("keep.txt"))?, "keep v1\n");
    assert!(!b.join("a-new.txt").exists(), "取り込み前には無かった");
    assert_eq!(std::fs::read_to_string(b.join("draft.txt"))?, "draft\n");
    // 履歴は動かさない（reset はしない）。戻す前の状態の復元点が新しくできている
    assert_eq!(get_head_commit(b), head_after_pull);
    assert!(refs(b).contains("refs/hikae/backup/undo-restore/"));
    Ok(())
}

#[test]
fn recover_uses_the_head_backup_when_nothing_was_unsaved() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    std::fs::write(a.join("keep.txt"), "keep v2 (A)\n")?;
    ops_named(Some("PC-A")).save(a, "A change")?;
    ops_named(Some("PC-A")).upload(a)?;

    let ops_b = ops_fixed(Some("PC-B"));
    assert!(matches!(ops_b.pull(b)?, PullOutcome::FastForwarded));
    assert_eq!(
        std::fs::read_to_string(b.join("keep.txt"))?,
        "keep v2 (A)\n"
    );

    let restored = ops_b.recover_interrupted(b, "pull", FIXED_UNIX)?;
    assert!(
        restored.starts_with("refs/hikae/backup/pull/"),
        "{restored}"
    );
    assert_eq!(std::fs::read_to_string(b.join("keep.txt"))?, "keep v1\n");
    Ok(())
}

#[test]
fn recover_aborts_a_half_finished_merge_and_returns_to_the_pre_pull_state() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    std::fs::write(a.join("conflict.txt"), "line1 from A\nline2\nline3\n")?;
    ops_named(Some("PC-A")).save(a, "A edit")?;
    ops_named(Some("PC-A")).upload(a)?;
    std::fs::write(b.join("conflict.txt"), "line1 from B\nline2\nline3\n")?;
    let ops_b = ops_fixed(Some("PC-B"));
    ops_b.save(b, "B edit")?;
    let head_before = get_head_commit(b);

    // 取り込みが競合で止まったまま（MERGE_HEAD が残っている）
    assert!(matches!(ops_b.pull(b)?, PullOutcome::Conflicted { .. }));
    assert!(std::fs::read_to_string(b.join("conflict.txt"))?.contains("<<<<<<<"));

    ops_b.recover_interrupted(b, "pull", FIXED_UNIX)?;
    assert_eq!(
        std::fs::read_to_string(b.join("conflict.txt"))?,
        "line1 from B\nline2\nline3\n"
    );
    assert!(ops_b.conflicts(b)?.is_empty());
    assert_ne!(
        run_git(b, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).0,
        0
    );
    assert_eq!(get_head_commit(b), head_before);
    // 取り込みをやり直せる
    assert!(matches!(ops_b.pull(b)?, PullOutcome::Conflicted { .. }));
    Ok(())
}

#[test]
fn recover_does_nothing_without_a_restore_point_or_for_other_operations() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    let ops_b = ops_fixed(Some("PC-B"));
    std::fs::write(b.join("draft.txt"), "unsaved\n")?;
    let before_refs = refs(b);

    // この操作の復元点がない
    assert!(matches!(
        ops_b.recover_interrupted(b, "pull", FIXED_UNIX),
        Err(OpsError::RestorePointNotFound)
    ));
    // 復旧の対象でない操作
    assert!(matches!(
        ops_b.recover_interrupted(b, "restore", FIXED_UNIX),
        Err(OpsError::InvalidInput(_))
    ));
    // 失敗した呼び出しは復元点も作らない
    assert_eq!(refs(b), before_refs);
    // 開始より前の復元点は使わない
    std::fs::write(pcs.pc_a.join("keep.txt"), "v2\n")?;
    ops_named(Some("PC-A")).save(&pcs.pc_a, "v2")?;
    ops_named(Some("PC-A")).upload(&pcs.pc_a)?;
    ops_b.pull(b)?;
    assert!(matches!(
        ops_b.recover_interrupted(b, "pull", FIXED_UNIX + 1),
        Err(OpsError::RestorePointNotFound)
    ));

    // 何も変更していない: 失敗した呼び出しは復元点も作らず、ファイルもそのまま
    assert!(!refs(b).is_empty() && refs(b) != before_refs);
    assert_eq!(std::fs::read_to_string(b.join("draft.txt"))?, "unsaved\n");
    Ok(())
}

// ---------------------------------------------------------------- 取り込み前の自動保存のサイズ検査

#[test]
fn pull_stops_before_changing_anything_when_the_auto_save_has_big_files() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    std::fs::write(a.join("keep.txt"), "keep v2 (A)\n")?;
    ops_named(Some("PC-A")).save(a, "A change")?;
    ops_named(Some("PC-A")).upload(a)?;

    // PC-B には大きい未保存ファイルがある（警告 1 件、保存不可 1 件）
    std::fs::write(b.join("mid.psd"), vec![b'x'; 2_000])?;
    std::fs::write(b.join("huge.mp4"), vec![b'x'; 6_000])?;
    let ops_b = ops_named(Some("PC-B"));
    let head = get_head_commit(b);

    let PullOutcome::NeedsSizeDecision(found) = ops_b.pull_with(b, LIMITS)? else {
        panic!("expected NeedsSizeDecision");
    };
    assert_eq!(found.blocked.len(), 1);
    assert_eq!(found.blocked[0].path, "huge.mp4");
    assert_eq!(found.warned.len(), 1);
    assert_eq!(found.warned[0].path, "mid.psd");

    // 何も変更していない: 復元点なし、保存なし、取り込みなし、ファイルはそのまま
    assert!(refs(b).trim().is_empty(), "{}", refs(b));
    assert_eq!(get_head_commit(b), head);
    assert_eq!(std::fs::read_to_string(b.join("keep.txt"))?, "keep v1\n");
    assert!(b.join("mid.psd").exists() && b.join("huge.mp4").exists());
    assert!(run_git(b, &["diff", "--cached", "--name-only"])
        .1
        .trim()
        .is_empty());

    // 大きいファイルを片付ければ取り込める
    std::fs::remove_file(b.join("huge.mp4"))?;
    std::fs::remove_file(b.join("mid.psd"))?;
    assert!(matches!(
        ops_b.pull_with(b, LIMITS)?,
        PullOutcome::FastForwarded
    ));
    Ok(())
}

#[test]
fn upload_that_would_pull_reports_the_size_decision_and_changes_nothing() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    let ops_b = ops_named(Some("PC-B"));
    // 双方が先に進んでいる（push は拒否される）
    std::fs::write(b.join("b-only.txt"), "b\n")?;
    ops_b.save(b, "B change")?;
    std::fs::write(a.join("keep.txt"), "keep v2 (A)\n")?;
    ops_named(Some("PC-A")).save(a, "A change")?;
    ops_named(Some("PC-A")).upload(a)?;
    std::fs::write(b.join("mid.psd"), vec![b'x'; 2_000])?;
    let head = get_head_commit(b);
    let refs_before = refs(b);

    let out = ops_b.upload_with(b, LIMITS)?;
    let UploadOutcome::NeedsSizeDecision(found) = out else {
        panic!("expected NeedsSizeDecision, got {out:?}");
    };
    assert_eq!(found.warned.len(), 1);
    assert_eq!(get_head_commit(b), head);
    assert_eq!(refs(b), refs_before);
    assert_eq!(std::fs::read_to_string(b.join("keep.txt"))?, "keep v1\n");
    // アップロードもされていない
    let (_, remote_log, _) = run_git(&pcs.bare, &["log", "--format=%s", "main"]);
    assert!(!remote_log.contains("B change"));
    Ok(())
}

// ---------------------------------------------------------------- 競合解消後の書き戻し

#[test]
fn resolving_a_conflicted_pull_also_restores_files_the_other_pc_stopped_tracking() -> TestResult {
    let pcs = two_pcs()?;
    let (a, b) = (&pcs.pc_a, &pcs.pc_b);
    let original = std::fs::read(b.join("secret.bin"))?;

    // PC-A: secret.bin を保存対象から外し、conflict.txt も変更してアップロード
    std::fs::write(a.join(".gitignore"), "/secret.bin\n")?;
    let (code, _, err) = run_git(a, &["rm", "--cached", "secret.bin"]);
    assert_eq!(code, 0, "{err}");
    std::fs::write(a.join("conflict.txt"), "line1 from A\nline2\nline3\n")?;
    ops_named(Some("PC-A")).save(a, "exclude secret and edit")?;
    ops_named(Some("PC-A")).upload(a)?;

    // PC-B: conflict.txt を別に変更して保存し、取り込むとぶつかる
    std::fs::write(b.join("conflict.txt"), "line1 from B\nline2\nline3\n")?;
    let ops_b = ops_named(Some("PC-B"));
    ops_b.save(b, "B edit")?;
    assert!(matches!(ops_b.pull(b)?, PullOutcome::Conflicted { .. }));
    assert!(
        !b.join("secret.bin").exists(),
        "競合で止まった取り込みでは、相手が外したファイルは作業フォルダから消えている"
    );

    ops_b.resolve(
        b,
        &[("conflict.txt".to_string(), Choice::Mine)],
        false,
        "まとめた",
    )?;

    // 取り込み前の内容が書き戻され、保存対象外のまま残る
    assert_eq!(std::fs::read(b.join("secret.bin"))?, original);
    let (_, tracked, _) = run_git(b, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tracked.lines().any(|l| l == "secret.bin"));
    assert_eq!(run_git(b, &["check-ignore", "-q", "secret.bin"]).0, 0);
    assert!(run_git(b, &["status", "--porcelain"]).1.trim().is_empty());
    assert_eq!(
        std::fs::read_to_string(b.join("conflict.txt"))?,
        "line1 from B\nline2\nline3\n"
    );
    Ok(())
}

// ---------------------------------------------------------------- 復元点の署名

#[test]
fn restore_point_snapshots_are_signed_with_the_given_user() -> TestResult {
    let pcs = two_pcs()?;
    let b = &pcs.pc_b;
    std::fs::write(b.join("keep.txt"), "unsaved\n")?;

    let author = |repo: &Path| {
        let (_, refname, _) = run_git(
            repo,
            &[
                "for-each-ref",
                "--sort=-refname",
                "--count=1",
                "--format=%(refname)",
                "refs/hikae/snapshots/",
            ],
        );
        run_git(repo, &["log", "-1", "--format=%an <%ae>", refname.trim()])
            .1
            .trim()
            .to_string()
    };
    // 未ログイン: 固定の Hikae
    ops_named(Some("PC-B")).save(b, "unsaved to saved")?;
    assert!(refs(b).contains("refs/hikae/backup/save/"));
    assert_eq!(author(b), "Hikae <hikae@localhost>");

    // ログイン済み: 実ユーザー
    std::fs::write(b.join("keep.txt"), "unsaved 2\n")?;
    ops_named(Some("PC-B"))
        .with_signing_user(Some((583231, "octocat")))
        .save(b, "signed")?;
    assert_eq!(
        author(b),
        "octocat <583231+octocat@users.noreply.github.com>"
    );
    Ok(())
}
