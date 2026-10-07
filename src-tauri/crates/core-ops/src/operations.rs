// 各操作の実装。設計書 4.1～4.5 に従う。

use crate::memo::{self, MemoChange, MemoChangeKind, MemoLabels};
use crate::models::*;
use core_git::GitRunner;
use core_safety::{create_backup_ref, create_restore_point};
use std::path::Path;
use time::OffsetDateTime;

/// リポジトリを初期化。remote_url があれば remote add。
pub(crate) fn init_project(
    runner: &GitRunner,
    dir: &Path,
    remote_url: Option<&str>,
    identity: &Identity,
    _now: OffsetDateTime,
) -> Result<(), OpsError> {
    // git init の cwd になるので、先にディレクトリを作る（既存ならそのまま使う）
    std::fs::create_dir_all(dir)?;

    // init -b main
    runner.run_ok(dir, &["init", "-b", "main"])?;

    // local config を設定
    runner.run_ok(dir, &["config", "--local", "core.autocrlf", "false"])?;
    runner.run_ok(
        dir,
        &["config", "--local", "core.precomposeUnicode", "true"],
    )?;
    runner.run_ok(dir, &["config", "--local", "user.name", &identity.name])?;
    runner.run_ok(dir, &["config", "--local", "user.email", &identity.email])?;

    // remote を追加
    if let Some(url) = remote_url {
        runner.run_ok(dir, &["remote", "add", "origin", url])?;
    }

    Ok(())
}

/// URL から clone し、local config を設定。
pub(crate) fn clone_project(
    runner: &GitRunner,
    url: &str,
    dest: &Path,
    identity: &Identity,
    _now: OffsetDateTime,
) -> Result<(), OpsError> {
    // dest の親ディレクトリを cwd として clone を実行
    let parent = dest
        .parent()
        .ok_or_else(|| OpsError::Unexpected("dest has no parent directory".to_string()))?;

    let dest_name = dest
        .file_name()
        .ok_or_else(|| OpsError::Unexpected("dest has no file name".to_string()))?
        .to_str()
        .ok_or_else(|| OpsError::Unexpected("dest name is not valid utf-8".to_string()))?;

    std::fs::create_dir_all(parent)?;
    runner.run_ok(parent, &["clone", url, dest_name])?;

    // local config を設定
    runner.run_ok(dest, &["config", "--local", "core.autocrlf", "false"])?;
    runner.run_ok(
        dest,
        &["config", "--local", "core.precomposeUnicode", "true"],
    )?;
    runner.run_ok(dest, &["config", "--local", "user.name", &identity.name])?;
    runner.run_ok(dest, &["config", "--local", "user.email", &identity.email])?;

    Ok(())
}

/// 変更をコミット。未保存変更がなければ NothingToSave。
pub(crate) fn save(
    runner: &GitRunner,
    repo: &Path,
    memo: &str,
    now: OffsetDateTime,
) -> Result<SaveOutcome, OpsError> {
    // マージ競合中でないか確認
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    // 変更がないか確認（未追跡ファイルも変更として数える）
    let has_changes = !read_status(runner, repo)?.entries.is_empty();

    if !has_changes {
        return Ok(SaveOutcome::NothingToSave);
    }

    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let restore_point_internal = create_restore_point(runner, repo, &branch, "save", now)?;

    // add -A
    runner.run_ok(repo, &["add", "-A"])?;

    // commit
    let _commit_output = runner.run_ok(repo, &["-c", "core.hooksPath=", "commit", "-m", memo])?;

    // commit ハッシュを抽出（出力から取得）
    let commit_hash = runner.run(repo, &["rev-parse", "HEAD"])?;
    let commit = String::from_utf8_lossy(&commit_hash.stdout)
        .trim()
        .to_string();

    let restore_point = RestorePointInfo {
        snapshot_ref: restore_point_internal
            .snapshot
            .as_ref()
            .map(|s| s.ref_name.clone()),
        backup_ref: restore_point_internal.backup_ref,
    };

    Ok(SaveOutcome::Saved {
        commit,
        restore_point,
    })
}

/// HEAD と upstream の (アップロード待ち, 取り込み待ち) の件数を返す。
fn ahead_behind(runner: &GitRunner, repo: &Path) -> Result<(i32, i32), OpsError> {
    let count_output = runner.run_ok(
        repo,
        &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
    )?;
    let count_str = String::from_utf8_lossy(&count_output.stdout)
        .trim()
        .to_string();
    let parts: Vec<&str> = count_str.split_whitespace().collect();
    if parts.len() >= 2 {
        Ok((
            parts[0].parse::<i32>().unwrap_or(0),
            parts[1].parse::<i32>().unwrap_or(0),
        ))
    } else {
        Ok((0, 0))
    }
}

/// upstream から取り込む。未保存変更があれば自動保存してから pull。
pub(crate) fn pull(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
    labels: &Labels,
) -> Result<PullOutcome, OpsError> {
    // マージ競合中でないか確認
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    // upstream の設定を確認
    let upstream_output = runner.run(repo, &["rev-parse", "--abbrev-ref", "@{u}"])?;
    if upstream_output.code != 0 {
        return Ok(PullOutcome::NoUpstream);
    }

    // fetch は remote-tracking ref を更新するだけで、作業フォルダ・インデックスは変えない。
    // 復元点より先に実行し、変更が必要かどうかの判定材料にする
    runner.run_ok(repo, &["fetch", "--prune", "origin"])?;

    let has_unsaved = !read_status(runner, repo)?.entries.is_empty();
    let (_, behind_before) = ahead_behind(runner, repo)?;

    // 取り込む内容も自動保存する内容も無ければ何も変更しないため、復元点は作らない
    if behind_before == 0 && !has_unsaved {
        return Ok(PullOutcome::UpToDate);
    }

    // 作業フォルダ・履歴を変更する前に復元点を 1 回だけ作る（未保存の変更もここに含まれる）
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    let _ = create_restore_point(runner, repo, &branch, "pull", now)?;

    // 未保存の変更があれば先に保存する
    if has_unsaved {
        runner.run_ok(repo, &["add", "-A"])?;
        runner.run_ok(
            repo,
            &[
                "-c",
                "core.hooksPath=",
                "commit",
                "-m",
                &labels.auto_save_memo,
            ],
        )?;
    }

    // 自動保存でアップロード待ちが増えうるため、改めて算出する
    let (ahead, behind) = ahead_behind(runner, repo)?;

    // 状態判定
    if behind == 0 {
        return Ok(PullOutcome::UpToDate);
    }

    if ahead == 0 {
        // fast-forward のみ
        runner.run_ok(repo, &["merge", "--ff-only", "@{u}"])?;
        return Ok(PullOutcome::FastForwarded);
    }

    // 双方に差がある場合は backup ref を作成してから merge
    let _ = create_backup_ref(runner, repo, "pre-merge", now)?;

    // merge --no-edit @{u}（終了コード 0, 1 を許容）
    let merge_output = runner.run_allow_codes(repo, &["merge", "--no-edit", "@{u}"], &[0, 1])?;

    if merge_output.code == 0 {
        // merge 成功
        let commit_output = runner.run(repo, &["rev-parse", "HEAD"])?;
        let commit = String::from_utf8_lossy(&commit_output.stdout)
            .trim()
            .to_string();
        return Ok(PullOutcome::Merged { commit });
    }

    // merge 失敗（競合）
    let conflict_files = conflicts(runner, repo)?;
    Ok(PullOutcome::Conflicted {
        files: conflict_files,
    })
}

/// 現在の競合ファイルを列挙
pub(crate) fn conflicts(runner: &GitRunner, repo: &Path) -> Result<Vec<ConflictFile>, OpsError> {
    let status = read_status(runner, repo)?;
    let mut conflicts = Vec::new();

    for entry in status.unmerged() {
        let core_git::StatusKind::Unmerged { xy } = &entry.kind else {
            continue;
        };
        // XY: UU=両方変更, AA=両方追加, DU=この PC で削除, UD=クラウドで削除, DD=両方削除
        let kind = match xy.as_str() {
            "UU" => ConflictKind::BothModified,
            "AA" => ConflictKind::BothAdded,
            "DU" => ConflictKind::DeletedByUs,
            "UD" => ConflictKind::DeletedByThem,
            "DD" => ConflictKind::BothDeleted,
            other => {
                return Err(OpsError::Unexpected(format!(
                    "unsupported conflict type {other}: {}",
                    entry.path
                )))
            }
        };
        conflicts.push(ConflictFile {
            path: entry.path.clone(),
            kind,
            // この PC 側は HEAD、クラウド側は取り込み中の MERGE_HEAD
            this_saved_at: last_saved_at(runner, repo, "HEAD", &entry.path),
            cloud_saved_at: last_saved_at(runner, repo, "MERGE_HEAD", &entry.path),
        });
    }

    Ok(conflicts)
}

/// 指定の参照でそのパスを最後に変更したコミットの時刻（Unix 秒）を返す。
/// 参照が無い・履歴が空・解析できない場合は None（表示側で日時を隠す）。
fn last_saved_at(runner: &GitRunner, repo: &Path, rev: &str, path: &str) -> Option<i64> {
    let out = runner
        .run_ok(
            repo,
            &["log", "--max-count=1", "--format=%ct", rev, "--", path],
        )
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `status --porcelain=v2 -z --branch` を実行して解析する
fn read_status(runner: &GitRunner, repo: &Path) -> Result<core_git::StatusV2, OpsError> {
    let out = runner.run_ok(repo, &["status", "--porcelain=v2", "-z", "--branch"])?;
    core_git::parse_status_v2(&out.stdout).map_err(|e| OpsError::Unexpected(e.to_string()))
}

/// 履歴一覧を取得。最新順。refs/hikae/snapshots/ の自動保存は区別される。
pub(crate) fn history(
    runner: &GitRunner,
    repo: &Path,
    max_count: usize,
) -> Result<Vec<HistoryEntry>, OpsError> {
    // git log の出力形式: "%H %ai %s" （ハッシュ、タイムスタンプ、メッセージ）
    let cmd = [
        "log",
        "--format=%H|%ai|%s",
        "--max-count",
        &max_count.to_string(),
        "HEAD",
    ];
    let out = runner.run_ok(repo, &cmd)?;
    let log_output = String::from_utf8_lossy(&out.stdout);

    let mut entries = Vec::new();
    for line in log_output.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 {
            continue;
        }

        let commit_full = parts[0];
        let commit = commit_full.chars().take(7).collect::<String>();
        let timestamp = parts[1].to_string();
        let message = parts[2..].join("|");

        // デフォルトでは手動の保存として扱う
        let snapshot_ref = None;

        // ファイル変更数を取得
        let stat_out = runner.run_ok(
            repo,
            &["diff-tree", "--numstat", "-r", "--max-count=1", commit_full],
        )?;
        let stat_output = String::from_utf8_lossy(&stat_out.stdout);
        let changed_files_count = stat_output.lines().count() as u32;

        entries.push(HistoryEntry {
            commit,
            timestamp,
            message,
            snapshot_ref,
            changed_files_count,
        });
    }

    Ok(entries)
}

/// 特定時点のファイル一覧を取得。
pub(crate) fn list_files_at(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
) -> Result<Vec<FileInHistory>, OpsError> {
    // 先頭が `-` の値はオプションとして解釈されるため受け付けない
    if commit.is_empty() || commit.starts_with('-') {
        return Err(OpsError::Unexpected("invalid commit".to_string()));
    }

    // `-z` によりパスは引用符なしの生のバイト列で届く（日本語名・空白を含む名前も正しく扱える）
    let out = runner.run_ok(repo, &["ls-tree", "-r", "-z", "-l", commit])?;
    Ok(parse_ls_tree_long(&out.stdout))
}

/// `ls-tree -r -z -l` の出力を解析する。
/// 各エントリは `<mode> <type> <oid> <size>\t<path>` が NUL で区切られる。
/// サブモジュール（commit）など blob 以外は一覧に含めない。
fn parse_ls_tree_long(stdout: &[u8]) -> Vec<FileInHistory> {
    let mut files = Vec::new();
    for entry in stdout.split(|b| *b == 0) {
        if entry.is_empty() {
            continue;
        }
        let Some(tab) = entry.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let meta = String::from_utf8_lossy(&entry[..tab]);
        let path = String::from_utf8_lossy(&entry[tab + 1..]).into_owned();
        let mut fields = meta.split_whitespace();
        let (Some(mode), Some(kind), Some(_oid), Some(size)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if kind != "blob" {
            continue;
        }
        files.push(FileInHistory {
            path,
            mode: mode.to_string(),
            size: size.parse().unwrap_or(0),
            is_tracked: true,
        });
    }
    files
}

/// いまの作業状態（未保存の変更）からメモの案を作る（設計書 4.1 手順 3 / 10.3）。
/// 作業フォルダ・インデックスは変更しない（`status` の読み取りのみ）。
pub(crate) fn suggest_memo_for(
    runner: &GitRunner,
    repo: &Path,
    labels: &MemoLabels,
) -> Result<String, OpsError> {
    // 未追跡ファイルをフォルダ単位にまとめず 1 件ずつ列挙する
    let out = runner.run_ok(
        repo,
        &["status", "--porcelain=v2", "-z", "-uall", "--no-renames"],
    )?;
    let status =
        core_git::parse_status_v2(&out.stdout).map_err(|e| OpsError::Unexpected(e.to_string()))?;
    let changes = memo_changes_from_status(repo, &status);
    Ok(memo::suggest_memo(&changes, labels))
}

/// `status` のエントリを、メモ生成の入力へ変換する。
/// ファイルの大きさを変更量の目安にする（削除は 0）。
fn memo_changes_from_status(repo: &Path, status: &core_git::StatusV2) -> Vec<MemoChange> {
    use core_git::{StatusCode, StatusKind};

    let mut changes = Vec::new();
    for entry in &status.entries {
        let (kind, old_path) = match &entry.kind {
            StatusKind::Ignored => continue,
            StatusKind::Untracked => (MemoChangeKind::Added, None),
            StatusKind::Unmerged { .. } => (MemoChangeKind::Modified, None),
            StatusKind::Rename {
                original_path,
                copy,
                ..
            } => {
                if *copy {
                    (MemoChangeKind::Added, None)
                } else {
                    (MemoChangeKind::Renamed, Some(original_path.clone()))
                }
            }
            StatusKind::Change { index, worktree } => {
                if *worktree == StatusCode::Deleted
                    || (*index == StatusCode::Deleted && *worktree == StatusCode::Unmodified)
                {
                    (MemoChangeKind::Deleted, None)
                } else if *index == StatusCode::Added || *worktree == StatusCode::Added {
                    (MemoChangeKind::Added, None)
                } else {
                    (MemoChangeKind::Modified, None)
                }
            }
        };
        let weight = if kind == MemoChangeKind::Deleted {
            0
        } else {
            std::fs::metadata(repo.join(&entry.path))
                .map(|m| m.len())
                .unwrap_or(0)
        };
        changes.push(MemoChange {
            kind,
            path: entry.path.clone(),
            old_path,
            weight,
        });
    }
    changes
}

/// 復元点（`refs/hikae/backup/` と `refs/hikae/snapshots/`）の ref 名を列挙する。
/// 操作の前後で差を取り、その操作が作った復元点をジャーナルへ記録するために使う。
pub(crate) fn restore_point_refs(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, OpsError> {
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hikae/backup/",
            "refs/hikae/snapshots/",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// 2つの時点の差分を取得（行単位）。
pub(crate) fn diff_with(
    runner: &GitRunner,
    repo: &Path,
    from: &str,
    to: &str,
    path: Option<&str>,
) -> Result<Vec<DiffLine>, OpsError> {
    let mut cmd = vec!["diff", "-U3", from, to];
    if let Some(p) = path {
        cmd.push("--");
        cmd.push(p);
    }

    let out = runner.run_ok(repo, &cmd)?;
    let diff_output = String::from_utf8_lossy(&out.stdout);

    // 簡易的な diff 解析（本来は proper な parser が必要）
    let mut lines = Vec::new();
    let mut line_num_old = 0;
    let mut line_num_new = 0;

    for line in diff_output.lines() {
        if line.starts_with("@@") {
            // ハンク開始行
            continue;
        }

        if let Some(content) = line.strip_prefix("-") {
            if !line.starts_with("---") {
                lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    line_number_old: Some(line_num_old),
                    line_number_new: None,
                    content: content.to_string(),
                });
                line_num_old += 1;
            }
        } else if let Some(content) = line.strip_prefix("+") {
            if !line.starts_with("+++") {
                lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    line_number_old: None,
                    line_number_new: Some(line_num_new),
                    content: content.to_string(),
                });
                line_num_new += 1;
            }
        } else if let Some(content) = line.strip_prefix(" ") {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                line_number_old: Some(line_num_old),
                line_number_new: Some(line_num_new),
                content: content.to_string(),
            });
            line_num_old += 1;
            line_num_new += 1;
        }
    }

    Ok(lines)
}

/// 元に戻す操作のプレビュー（影響ファイル一覧）。
pub(crate) fn restore_preview(
    runner: &GitRunner,
    repo: &Path,
    target_commit: &str,
) -> Result<RestorePreview, OpsError> {
    // 現在の状態と target_commit の差分を取得
    let diff_out = runner.run_ok(repo, &["diff", "--name-status", target_commit])?;
    let diff_output = String::from_utf8_lossy(&diff_out.stdout);

    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut created = Vec::new();

    for line in diff_output.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let status = parts[0];
        let path = parts[1];

        match status {
            "M" => {
                // 変更：サイズは 0 で固定（詳細は phase 2）
                modified.push(RestoreFileChange {
                    path: path.to_string(),
                    size_from: 0,
                    size_to: 0,
                });
            }
            "A" => {
                // 追加（元に戻すと削除される）
                deleted.push(path.to_string());
            }
            "D" => {
                // 削除（元に戻すと復活する）
                created.push(path.to_string());
            }
            _ => {} // R, C などは簡略化のため無視
        }
    }

    Ok(RestorePreview {
        modified,
        deleted,
        created,
    })
}

/// 指定の時点へ復元。復元前に復元点を作成し、指定時点のファイル状態に復元。
pub(crate) fn restore(
    runner: &GitRunner,
    repo: &Path,
    target_commit: &str,
    now: OffsetDateTime,
) -> Result<(), OpsError> {
    // マージ競合中でないか確認
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let _ = create_restore_point(runner, repo, &branch, "restore", now)?;

    // restore --source で指定時点のファイル状態に復元（未追跡ファイルは消さない）
    // 注意: restore --source は reset と異なり、staged / worktree の両方を復元する
    runner.run_ok(
        repo,
        &[
            "restore",
            "--source",
            target_commit,
            "--staged",
            "--worktree",
            "--",
            ":",
        ],
    )?;

    Ok(())
}

/// 競合を解消
pub(crate) fn resolve(
    runner: &GitRunner,
    repo: &Path,
    choices: &[(String, Choice)],
    keep_other_copy: bool,
    message: &str,
    now: OffsetDateTime,
    labels: &Labels,
) -> Result<ResolveOutcome, OpsError> {
    // 現在の競合ファイルを確認
    let current_conflicts = conflicts(runner, repo)?;
    if current_conflicts.is_empty() {
        return Err(OpsError::Unexpected("no conflicts to resolve".to_string()));
    }

    // choices が全競合をカバーしているか確認
    if current_conflicts.len() != choices.len() {
        return Err(OpsError::Unexpected(
            "choices must cover all conflicts".to_string(),
        ));
    }

    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let _ = create_restore_point(runner, repo, &branch, "resolve", now)?;

    let mut copies = Vec::new();

    for (path, choice) in choices {
        // 対応するファイルの競合情報を取得
        let file_conflict = current_conflicts
            .iter()
            .find(|f| &f.path == path)
            .ok_or_else(|| OpsError::Unexpected(format!("file {} not in conflict", path)))?;

        match file_conflict.kind {
            ConflictKind::BothModified | ConflictKind::BothAdded => {
                // keep_other_copy の場合は先に使わない版を保存
                if keep_other_copy {
                    let other_stage = if *choice == Choice::Mine { "3" } else { "2" };
                    let show_output =
                        runner.run(repo, &["show", &format!(":{other_stage}:{}", path)])?;

                    if show_output.code == 0 {
                        // 使わなかった側の版に付けるラベル（Mine を選んだなら破棄側はクラウドの版）
                        let label = if *choice == Choice::Mine {
                            &labels.copy_theirs
                        } else {
                            &labels.copy_mine
                        };
                        let copy_path = make_copy_path(repo, path, label, now);
                        std::fs::write(repo.join(&copy_path), &show_output.stdout)?;
                        copies.push(copy_path);
                    }
                }

                let choice_arg = if *choice == Choice::Mine {
                    "--ours"
                } else {
                    "--theirs"
                };
                runner.run_ok(repo, &["checkout", choice_arg, "--", path])?;
                runner.run_ok(repo, &["add", "--", path])?;
            }

            ConflictKind::DeletedByUs => {
                match choice {
                    Choice::Mine => {
                        // 削除を維持
                        runner.run_ok(repo, &["rm", "--cached", "--", path])?;
                    }
                    Choice::Theirs => {
                        // クラウドの版を残す
                        let show_output = runner.run(repo, &["show", &format!(":3:{}", path)])?;
                        if show_output.code == 0 {
                            std::fs::write(repo.join(path), &show_output.stdout)?;
                        }
                        runner.run_ok(repo, &["add", "--", path])?;
                    }
                }
            }

            ConflictKind::DeletedByThem => {
                match choice {
                    Choice::Mine => {
                        // 残す
                        runner.run_ok(repo, &["add", "--", path])?;
                    }
                    Choice::Theirs => {
                        // 削除を受け入れる
                        runner.run_ok(repo, &["rm", "--cached", "--", path])?;
                        let _ = std::fs::remove_file(repo.join(path));
                    }
                }
            }

            ConflictKind::BothDeleted => {
                runner.run_ok(repo, &["rm", "--cached", "--", path])?;
            }
        }
    }

    // commit
    runner.run_ok(repo, &["-c", "core.hooksPath=", "commit", "-m", message])?;

    // 確認
    let remaining_conflicts = conflicts(runner, repo)?;
    if !remaining_conflicts.is_empty() {
        return Err(OpsError::Unexpected(
            "conflicts remain after resolution".to_string(),
        ));
    }

    let commit_output = runner.run(repo, &["rev-parse", "HEAD"])?;
    let commit = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    Ok(ResolveOutcome { commit, copies })
}

/// merge をキャンセル
pub(crate) fn abort_merge(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
) -> Result<(), OpsError> {
    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let _ = create_restore_point(runner, repo, &branch, "abort-merge", now)?;

    // merge --abort
    runner.run_ok(repo, &["merge", "--abort"])?;

    Ok(())
}

/// upstream に push。拒否されたら pull して再試行。
pub(crate) fn upload(
    runner: &GitRunner,
    repo: &Path,
    now: OffsetDateTime,
    labels: &Labels,
) -> Result<UploadOutcome, OpsError> {
    // マージ競合中でないか確認
    let current_conflicts = conflicts(runner, repo)?;
    if !current_conflicts.is_empty() {
        return Err(OpsError::Conflict(current_conflicts));
    }

    // sync_state で判定
    let sync = sync_state(runner, repo)?;

    if !sync.has_upstream {
        // upstream 未設定：push -u
        runner.run_ok(repo, &["push", "-u", "origin", "HEAD"])?;
        return Ok(UploadOutcome::Pushed);
    }

    if sync.ahead == 0 && sync.behind == 0 {
        return Ok(UploadOutcome::NothingToUpload);
    }

    // push を試行
    let push_output = runner.run(repo, &["push", "origin", "HEAD"])?;

    if push_output.code == 0 {
        return Ok(UploadOutcome::Pushed);
    }

    // push が拒否された。stderr を確認
    let stderr = &push_output.stderr;
    if stderr.contains("non-fast-forward")
        || stderr.contains("rejected")
        || stderr.contains("fetch first")
    {
        // pull を実行
        let pull_result = pull(runner, repo, now, labels)?;

        match &pull_result {
            PullOutcome::Conflicted { files } => {
                return Ok(UploadOutcome::NeedsResolve(files.clone()));
            }
            PullOutcome::NoUpstream => {
                return Err(OpsError::Unexpected("upstream lost after pull".to_string()));
            }
            _ => {
                // pull 成功。再度 push を試行
                let push_retry = runner.run_ok(repo, &["push", "origin", "HEAD"])?;
                if push_retry.code == 0 {
                    return Ok(UploadOutcome::PulledThenPushed(pull_result));
                }
            }
        }
    }

    // push が再度失敗
    Err(OpsError::Git(core_git::GitError::Failed {
        code: push_output.code,
        stderr: push_output.stderr,
        args_summary: "push origin HEAD".to_string(),
    }))
}

/// 同期状態を返す
pub(crate) fn sync_state(runner: &GitRunner, repo: &Path) -> Result<SyncState, OpsError> {
    let status = read_status(runner, repo)?;
    let (ahead, behind) = status.branch.ab.unwrap_or((0, 0));
    Ok(SyncState {
        ahead,
        behind,
        has_upstream: status.branch.upstream.is_some(),
        dirty: !status.entries.is_empty(),
    })
}

/// 使わない版を別名で保存するときのパスを作る。
/// `<元のフォルダ>/<名前> (<ラベル> MM-DD).<拡張子>`。既存ファイルは上書きせず、連番を付ける。
fn make_copy_path(repo: &Path, path: &str, label: &str, now: OffsetDateTime) -> String {
    let date = format!("{:02}-{:02}", now.month() as u8, now.day());
    let p = Path::new(path);
    let dir = p.parent().and_then(|d| d.to_str()).unwrap_or("");
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    let join = |name: String| {
        if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        }
    };
    let mut candidate = join(format!("{stem} ({label} {date}){ext}"));
    let mut n = 2;
    while repo.join(&candidate).exists() {
        candidate = join(format!("{stem} ({label} {date} {n}){ext}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-10-07 12:00 UTC);

    #[test]
    fn copy_path_keeps_directory_extension_and_label() {
        let tmp = std::env::temp_dir();
        assert_eq!(
            make_copy_path(&tmp, "sub/メモ 1.txt", "cloud", NOW),
            "sub/メモ 1 (cloud 10-07).txt"
        );
        assert_eq!(
            make_copy_path(&tmp, "file", "this PC", NOW),
            "file (this PC 10-07)"
        );
    }

    #[test]
    fn copy_path_never_overwrites_existing_files() {
        let dir = std::env::temp_dir().join(format!("core-ops-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("a (cloud 10-07).txt"), "x").expect("write");
        let p = make_copy_path(&dir, "a.txt", "cloud", NOW);
        assert_eq!(p, "a (cloud 10-07 2).txt");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
