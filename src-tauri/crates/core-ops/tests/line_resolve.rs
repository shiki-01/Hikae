// 行ごとの解消（設計書 4.4 手順 5）の統合テスト。実 git と一時ディレクトリの実リポジトリを使う。
// 競合は PC-A と PC-B の実際の保存・取り込みで作る（モックは使わない）。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{new_restore_points, Choice, Identity, Ops, OpsError, PullOutcome};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 1 MiB を超えるファイル（両側で約 1.2 MiB になり、競合マーカー付きの全文が上限を超える）
const BIG: usize = 600 * 1024;

fn identity(name: &str) -> Identity {
    Identity {
        name: name.to_string(),
        email: format!("{name}@users.noreply.github.com"),
    }
}

fn ops_named(pc: &str) -> Ops {
    Ops::new(GitRunner::from_path_env()).with_pc_name(Some(pc.to_string()))
}

/// PC-B で取り込みの競合が残っている状態。
/// - `conflict.txt`: 両方変更（UTF-8 のテキスト）
/// - `bin.dat`: 両方変更（NUL を含む）
/// - `latin.txt`: 両方変更（UTF-8 として無効）
/// - `big.txt`: 両方追加（1 MiB 超）
/// - `new.txt`: 両方追加（テキスト）
/// - `gone.txt`: この PC（PC-B）で削除、クラウドで変更
struct Conflicted {
    _tmp: tempfile::TempDir,
    pc_b: PathBuf,
}

fn conflicted() -> Result<Conflicted, Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let bare = tmp.path().join("remote.git");
    let pc_a = tmp.path().join("pc-a");
    let pc_b = tmp.path().join("pc-b");
    create_bare_repo(&bare)?;
    let url = bare.to_string_lossy().to_string();

    let a = ops_named("PC-A");
    a.init_project(&pc_a, Some(&url), &identity("pc-a"))?;
    std::fs::write(pc_a.join("conflict.txt"), "line1\nline2\nline3\n")?;
    std::fs::write(pc_a.join("gone.txt"), "gone\n")?;
    std::fs::write(pc_a.join("bin.dat"), [0u8, 1, 2])?;
    std::fs::write(pc_a.join("latin.txt"), [0xffu8, b'\n'])?;
    a.save(&pc_a, "initial")?;
    a.upload(&pc_a)?;
    ops_named("PC-B").clone_project(&url, &pc_b, &identity("pc-b"))?;

    // PC-A の変更を保存してアップロードする
    std::fs::write(pc_a.join("conflict.txt"), "A1\nline2\nline3\n")?;
    std::fs::write(pc_a.join("bin.dat"), [0u8, b'A'])?;
    std::fs::write(pc_a.join("latin.txt"), [0xffu8, b'A', b'\n'])?;
    std::fs::write(pc_a.join("big.txt"), vec![b'a'; BIG])?;
    std::fs::write(pc_a.join("gone.txt"), "A changed gone\n")?;
    std::fs::write(pc_a.join("new.txt"), "A new\n")?;
    a.save(&pc_a, "A changes")?;
    a.upload(&pc_a)?;

    // PC-B の変更を保存する（gone.txt は削除する）
    let b = ops_named("PC-B");
    std::fs::write(pc_b.join("conflict.txt"), "B1\nline2\nline3\n")?;
    std::fs::write(pc_b.join("bin.dat"), [0u8, b'B'])?;
    std::fs::write(pc_b.join("latin.txt"), [0xfeu8, b'B', b'\n'])?;
    std::fs::write(pc_b.join("big.txt"), vec![b'b'; BIG])?;
    std::fs::write(pc_b.join("new.txt"), "B new\n")?;
    std::fs::remove_file(pc_b.join("gone.txt"))?;
    b.save(&pc_b, "B changes")?;

    match b.pull(&pc_b)? {
        PullOutcome::Conflicted { .. } => {}
        _ => return Err("expected Conflicted".into()),
    }
    Ok(Conflicted { _tmp: tmp, pc_b })
}

/// 競合の全ファイルを指定の選択で覆う choices を作る（edits のパスも含める）
fn choices(paths: &[&str], choice: Choice) -> Vec<(String, Choice)> {
    paths.iter().map(|p| (p.to_string(), choice)).collect()
}

const ALL: [&str; 6] = [
    "conflict.txt",
    "bin.dat",
    "latin.txt",
    "big.txt",
    "new.txt",
    "gone.txt",
];

fn read(repo: &Path, rel: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(repo.join(rel))?)
}

fn expect_invalid_input<T>(result: Result<T, OpsError>) -> TestResult {
    match result {
        Err(OpsError::InvalidInput(_)) => Ok(()),
        Err(other) => Err(format!("expected InvalidInput, got {other}").into()),
        Ok(_) => Err("expected InvalidInput, got Ok".into()),
    }
}

// ---------------------------------------------------------------- conflict_text

#[test]
fn conflict_text_returns_the_full_text_with_markers_for_text_conflicts() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");

    let text = ops
        .conflict_text(&c.pc_b, "conflict.txt")?
        .ok_or("expected the text of conflict.txt")?;
    assert_eq!(
        text,
        read(&c.pc_b, "conflict.txt")?,
        "作業フォルダの全文と一致する"
    );
    assert!(text.contains("<<<<<<< "), "{text}");
    assert!(text.contains("=======\n"), "{text}");
    assert!(text.contains(">>>>>>> "), "{text}");
    assert!(text.contains("A1") && text.contains("B1"), "{text}");

    // 両方追加のテキストも全文を返す
    assert!(ops.conflict_text(&c.pc_b, "new.txt")?.is_some());
    Ok(())
}

#[test]
fn conflict_text_is_none_for_binary_invalid_utf8_large_and_deleted_files() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    assert_eq!(ops.conflict_text(&c.pc_b, "bin.dat")?, None, "NUL を含む");
    assert_eq!(
        ops.conflict_text(&c.pc_b, "latin.txt")?,
        None,
        "UTF-8 として無効"
    );
    assert_eq!(
        ops.conflict_text(&c.pc_b, "big.txt")?,
        None,
        "1 MiB を超える"
    );
    assert_eq!(
        ops.conflict_text(&c.pc_b, "gone.txt")?,
        None,
        "片方が削除している（両方変更・両方追加ではない）"
    );
    Ok(())
}

#[test]
fn conflict_text_rejects_paths_that_are_not_in_conflict() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    expect_invalid_input(ops.conflict_text(&c.pc_b, "unknown.txt"))?;
    expect_invalid_input(ops.conflict_text(&c.pc_b, "../outside.txt"))?;
    expect_invalid_input(ops.conflict_text(&c.pc_b, ".git/config"))?;
    Ok(())
}

// ---------------------------------------------------------------- edits による解消

#[test]
fn edits_are_written_as_is_and_committed_as_a_two_parent_merge() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    let merged = "A1 と B1 をまとめた\nline2\nline3\n";

    ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[("conflict.txt".to_string(), merged.to_string())],
        false,
        "まとめた",
    )?;

    assert_eq!(read(&c.pc_b, "conflict.txt")?, merged, "作業フォルダの内容");
    assert_eq!(
        run_git(&c.pc_b, &["show", "HEAD:conflict.txt"]).1,
        merged,
        "commit の内容"
    );
    let parents = run_git(&c.pc_b, &["rev-list", "--parents", "-n", "1", "HEAD"]).1;
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "親が 2 つの merge: {parents}"
    );
    assert!(
        run_git(&c.pc_b, &["ls-files", "-u"]).1.is_empty(),
        "未解決の競合が残らない"
    );
    assert!(ops.conflicts(&c.pc_b)?.is_empty());
    Ok(())
}

#[test]
fn edited_text_with_markers_is_refused_before_anything_changes() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    let file_before = read(&c.pc_b, "conflict.txt")?;
    let unmerged_before = run_git(&c.pc_b, &["ls-files", "-u"]).1;
    let status_before = run_git(&c.pc_b, &["status", "--porcelain=v2", "-z"]).1;
    let refs_before = ops.restore_point_refs(&c.pc_b)?;

    for bad in [
        "keep\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> other\n",
        "a\r\n=======\r\nb\r\n",
        "a\n>>>>>>> other\n",
    ] {
        let result = ops.resolve(
            &c.pc_b,
            &choices(&ALL, Choice::Mine),
            &[("conflict.txt".to_string(), bad.to_string())],
            false,
            "x",
        );
        expect_invalid_input(result)?;
        assert_eq!(
            read(&c.pc_b, "conflict.txt")?,
            file_before,
            "作業フォルダが変わらない"
        );
        assert_eq!(run_git(&c.pc_b, &["ls-files", "-u"]).1, unmerged_before);
        assert_eq!(
            run_git(&c.pc_b, &["status", "--porcelain=v2", "-z"]).1,
            status_before
        );
        assert_eq!(
            ops.restore_point_refs(&c.pc_b)?,
            refs_before,
            "復元点を作らない"
        );
    }
    Ok(())
}

#[test]
fn edits_are_limited_to_text_conflicts_and_must_be_unique() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    let text = || "x\n".to_string();

    // 片方が削除している競合は編集できない
    expect_invalid_input(ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[("gone.txt".to_string(), text())],
        false,
        "x",
    ))?;
    // 競合一覧に無いパスは編集できない（choices は全競合を覆っている）
    expect_invalid_input(ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[("unknown.txt".to_string(), text())],
        false,
        "x",
    ))?;
    // 同じパスの編集が 2 件ある
    expect_invalid_input(ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[
            ("conflict.txt".to_string(), text()),
            ("conflict.txt".to_string(), text()),
        ],
        false,
        "x",
    ))?;

    // 何も解消されていない
    assert_eq!(ops.conflicts(&c.pc_b)?.len(), ALL.len());
    Ok(())
}

#[test]
fn choices_must_cover_every_conflict_exactly_once() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");

    // 1 件足りない
    let short = choices(&ALL[..5], Choice::Mine);
    assert!(matches!(
        ops.resolve(&c.pc_b, &short, &[], false, "x"),
        Err(OpsError::Unexpected(_))
    ));
    // 同じパスが 2 件あり、別の競合が覆われない
    let mut dup = choices(&ALL[..5], Choice::Mine);
    dup.push(("conflict.txt".to_string(), Choice::Mine));
    assert!(matches!(
        ops.resolve(&c.pc_b, &dup, &[], false, "x"),
        Err(OpsError::Unexpected(_))
    ));
    assert_eq!(ops.conflicts(&c.pc_b)?.len(), ALL.len());
    Ok(())
}

#[test]
fn edits_and_choices_can_be_mixed_and_edited_files_get_no_copy() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");

    // keep_other_copy を有効にする。edits のファイル（conflict.txt）には別名の版を作らない。
    // 選択のファイルのうち BothModified / BothAdded（bin.dat・latin.txt・big.txt・new.txt）の 4 件は作る
    let outcome = ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[("conflict.txt".to_string(), "mixed\n".to_string())],
        true,
        "mixed",
    )?;
    assert_eq!(outcome.copies.len(), 4, "copies: {:?}", outcome.copies);
    assert!(
        !outcome.copies.iter().any(|p| p.starts_with("conflict")),
        "copies: {:?}",
        outcome.copies
    );
    assert_eq!(read(&c.pc_b, "conflict.txt")?, "mixed\n");
    // Mine を選んだ new.txt は、この PC の版（B）のまま
    assert_eq!(read(&c.pc_b, "new.txt")?, "B new\n");
    // 別名として残ったクラウドの版（A）が書き出されている
    let copy = outcome
        .copies
        .iter()
        .find(|p| p.starts_with("new"))
        .ok_or("expected a copy of new.txt")?;
    assert_eq!(read(&c.pc_b, copy)?, "A new\n");
    assert!(ops.conflicts(&c.pc_b)?.is_empty());
    Ok(())
}

#[test]
fn the_restore_point_before_resolving_can_bring_back_the_conflicted_text() -> TestResult {
    let c = conflicted()?;
    let ops = ops_named("PC-B");
    let before_text = read(&c.pc_b, "conflict.txt")?;
    let refs_before = ops.restore_point_refs(&c.pc_b)?;

    ops.resolve(
        &c.pc_b,
        &choices(&ALL, Choice::Mine),
        &[("conflict.txt".to_string(), "resolved\n".to_string())],
        false,
        "x",
    )?;
    assert_eq!(read(&c.pc_b, "conflict.txt")?, "resolved\n");

    // 解消の操作で作られた復元点（スナップショット）に、解消前の競合マーカー付きの内容が残っている
    let refs_after = ops.restore_point_refs(&c.pc_b)?;
    let created = new_restore_points(&refs_before, &refs_after);
    let snapshot = created
        .snapshot_ref
        .ok_or("expected a snapshot restore point")?;
    let stored = run_git(&c.pc_b, &["show", &format!("{snapshot}:conflict.txt")]).1;
    assert_eq!(stored, before_text, "復元点の内容が解消前と一致する");

    // その復元点に戻すと、解消前の内容が作業フォルダに戻る
    ops.undo_restore(&c.pc_b, &snapshot)?;
    assert_eq!(read(&c.pc_b, "conflict.txt")?, before_text);
    Ok(())
}
