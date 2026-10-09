// 履歴のページ送り（`Ops::history_page`）の統合テスト（設計書 3章 S3、9.3 の `list_history`）。
// 実 git と一時ディレクトリの実リポジトリで 250 件超の履歴を作り、ページ単位の取得が
// 重複・欠落なく全体と一致することを確かめる。自動保存（重複する自動保存を含む）も混ぜる。
// 履歴の読み取りだけを行うため、作業フォルダ・インデックス・ref は変更しない（不変条件 3〜5 に触れない）。

mod common;

use common::*;
use core_git::GitRunner;
use core_ops::{AutoSnapshotOutcome, HistoryEntry, HistoryKind, Identity, Ops, SizeLimits};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 手動の保存の数（初期の保存を除く）
const MANUAL_COUNT: usize = 190;

fn ops() -> Ops {
    Ops::new(GitRunner::from_path_env())
}

fn git_ok(repo: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_git(repo, args);
    assert_eq!(code, 0, "git {args:?} failed: {stderr}");
    stdout.trim().to_string()
}

/// 手動の保存と自動保存を混ぜた履歴を作る。戻り値は (リポジトリ, 表示されるはずの自動保存の数,
/// 実際の自動保存の ref 名)。
///
/// - 3 件に 1 件は、未保存の内容を自動保存に控えてから、同じ内容を保存する（自動保存は重複するので出ない）
/// - 3 件に 1 件は、保存とは別の内容の自動保存を挟む（履歴に出る）
fn build_history(tmp: &Path) -> Result<(PathBuf, usize, String), Box<dyn std::error::Error>> {
    let repo = tmp.join("repo");
    ops().init_project(
        &repo,
        None,
        &Identity {
            name: "tester".to_string(),
            email: "tester@users.noreply.github.com".to_string(),
        },
    )?;
    write_test_file(&repo, "f.txt", "base")?;
    git_ok(&repo, &["add", "f.txt"]);
    git_ok(&repo, &["commit", "-q", "-m", "manual base"]);

    let mut visible_autos = 0;
    let mut stamp = 0u32;
    // 作業フォルダの内容を控えた自動保存を、復元点の名前空間に作る
    let mut snapshot = |repo: &Path| {
        // `stash create` は親が 2 つの commit を作るため、ツリーだけを取り出して、
        // 本物の自動保存と同じ「親が HEAD だけの commit」を作り直す
        let stash = git_ok(repo, &["stash", "create"]);
        assert!(!stash.is_empty(), "控える変更がない");
        let oid = git_ok(
            repo,
            &[
                "commit-tree",
                &format!("{stash}^{{tree}}"),
                "-p",
                "HEAD",
                "-m",
                "auto: test",
            ],
        );
        stamp += 1;
        let name = format!(
            "refs/hikae/snapshots/main/20260101T{:02}{:02}{:02}Z",
            stamp / 3600,
            (stamp / 60) % 60,
            stamp % 60
        );
        git_ok(repo, &["update-ref", &name, &oid]);
    };
    for i in 0..MANUAL_COUNT {
        match i % 3 {
            0 => {
                write_test_file(&repo, "f.txt", &format!("dup {i}"))?;
                snapshot(&repo);
            }
            1 => {
                write_test_file(&repo, "f.txt", &format!("tmp {i}"))?;
                snapshot(&repo);
                visible_autos += 1;
                write_test_file(&repo, "f.txt", &format!("v {i}"))?;
            }
            _ => write_test_file(&repo, "f.txt", &format!("v {i}"))?,
        }
        git_ok(&repo, &["commit", "-q", "-a", "-m", &format!("manual {i}")]);
    }

    // 実際の自動保存（一時インデックス）も 1 件混ぜる
    write_test_file(&repo, "f.txt", "unsaved latest")?;
    let AutoSnapshotOutcome::Created { snapshot_ref, .. } =
        ops().auto_snapshot(&repo, SizeLimits::default())?
    else {
        return Err("自動保存が作られなかった".into());
    };
    visible_autos += 1;
    Ok((repo, visible_autos, snapshot_ref))
}

fn ids(entries: &[HistoryEntry]) -> Vec<String> {
    entries.iter().map(|e| e.commit.clone()).collect()
}

/// 履歴の作成に時間がかかるため、1 つのテストで 3 つの観点を続けて確かめる
#[test]
fn history_paging_is_consistent_with_automatic_saves_mixed_in() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (repo, visible_autos, real_auto_ref) = build_history(tmp.path())?;
    pages_concatenate_to_the_full_history(&repo, visible_autos, &real_auto_ref)?;
    page_boundaries_and_out_of_range_offsets(&repo)?;
    last_saved_at_ignores_auto_saves(tmp.path(), &repo)?;
    Ok(())
}

fn pages_concatenate_to_the_full_history(
    repo: &Path,
    visible_autos: usize,
    real_auto_ref: &str,
) -> TestResult {
    let ops = ops();

    // 全体（1 回で取得）。手動の保存 + 初期の保存 + 重複しない自動保存
    let full = ops.history_page(repo, 0, 10_000)?;
    let manual = full
        .iter()
        .filter(|e| e.kind == HistoryKind::Manual)
        .count();
    let auto = full.iter().filter(|e| e.kind == HistoryKind::Auto).count();
    assert_eq!(manual, MANUAL_COUNT + 1, "手動の保存（最初の保存を含む）");
    assert_eq!(auto, visible_autos, "重複する自動保存は出ない");
    assert!(full.len() >= 250, "250 件以上の履歴: {}", full.len());
    let unique: HashSet<String> = ids(&full).into_iter().collect();
    assert_eq!(unique.len(), full.len(), "全体に重複がない");
    // 実際の自動保存（一時インデックスで作ったもの）も並んでいる
    let real_auto = git_ok(repo, &["rev-parse", "--short=7", real_auto_ref]);
    assert!(
        full.iter()
            .any(|e| e.kind == HistoryKind::Auto && e.commit == real_auto),
        "実際の自動保存が履歴に出る"
    );

    // どのページ幅でも、つなげると全体と一致する
    for page_size in [1usize, 64, 100, 200] {
        let mut joined = Vec::new();
        loop {
            let page = ops.history_page(repo, joined.len(), page_size)?;
            if page.is_empty() {
                break;
            }
            assert!(page.len() <= page_size);
            joined.extend(page);
            // 1 件ずつの確認は先頭の 15 件で十分（残りは他の幅で確かめる）
            if page_size == 1 && joined.len() >= 15 {
                break;
            }
        }
        if page_size == 1 {
            assert_eq!(ids(&joined), ids(&full[..joined.len()]), "幅 {page_size}");
        } else {
            assert_eq!(ids(&joined), ids(&full), "幅 {page_size}");
            // 中身（日時・メモ・種類・変更ファイル数）も同じ
            for (a, b) in joined.iter().zip(&full) {
                assert_eq!(a.timestamp, b.timestamp);
                assert_eq!(a.message, b.message);
                assert_eq!(a.kind, b.kind);
                assert_eq!(a.changed_files_count, b.changed_files_count);
            }
        }
    }
    Ok(())
}

fn page_boundaries_and_out_of_range_offsets(repo: &Path) -> TestResult {
    let ops = ops();
    let full = ops.history_page(repo, 0, 10_000)?;

    // 途中の窓は全体の同じ範囲と一致する
    let middle = ops.history_page(repo, 120, 30)?;
    assert_eq!(ids(&middle), ids(&full[120..150]));

    // 最後のページは端数になり、その先は空になる
    let tail = ops.history_page(repo, full.len() - 3, 200)?;
    assert_eq!(ids(&tail), ids(&full[full.len() - 3..]));
    assert!(ops.history_page(repo, full.len(), 200)?.is_empty());
    assert!(ops.history_page(repo, full.len() + 500, 200)?.is_empty());

    // limit が 0 なら空。従来の `history` は先頭からの取得と同じ
    assert!(ops.history_page(repo, 0, 0)?.is_empty());
    assert_eq!(ids(&ops.history(repo, 25)?), ids(&full[..25]));
    Ok(())
}

fn last_saved_at_ignores_auto_saves(tmp: &Path, repo: &Path) -> TestResult {
    let ops = ops();

    // 最終保存は、自動保存ではなく最新の手動の保存（HEAD）の日時
    let head_time = git_ok(repo, &["log", "-1", "--format=%aI", "HEAD"]);
    assert_eq!(
        ops.last_saved_at(repo)?.as_deref(),
        Some(head_time.as_str())
    );
    let recent = ops.history_page(repo, 0, 5)?;
    assert!(recent.iter().any(|e| e.kind == HistoryKind::Auto));

    // まだ保存が無いプロジェクトは None
    let fresh = tmp.join("fresh");
    ops.init_project(
        &fresh,
        None,
        &Identity {
            name: "x".to_string(),
            email: "x@users.noreply.github.com".to_string(),
        },
    )?;
    assert_eq!(ops.last_saved_at(&fresh)?, None);
    Ok(())
}
