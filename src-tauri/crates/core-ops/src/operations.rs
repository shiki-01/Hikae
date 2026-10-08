// 各操作の実装。設計書 4.1～4.5 に従う。

use crate::memo::{self, MemoChange, MemoChangeKind, MemoLabels};
use crate::models::*;
use crate::pc_name::{split_trailer, Meta};
use crate::size_check::{self, SaveOptions, SizeLimits};
use core_git::GitRunner;
use core_safety::create_backup_ref;
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

/// リポジトリ単位の署名を更新する（`--local` のみ）。
pub(crate) fn apply_identity(
    runner: &GitRunner,
    repo: &Path,
    identity: &Identity,
) -> Result<(), OpsError> {
    runner.run_ok(repo, &["config", "--local", "user.name", &identity.name])?;
    runner.run_ok(repo, &["config", "--local", "user.email", &identity.email])?;
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
    // 空でないフォルダには取得しない（既存のファイルを巻き込まない）
    if let Err(reason) = crate::clone_dest::check_clone_destination(dest) {
        return Err(OpsError::InvalidInput(format!(
            "clone destination is not usable: {reason:?}"
        )));
    }

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
///
/// 1. 新規・変更ファイルのサイズを検査する（設計書 4.1 手順 1）。決定が必要な大きいファイルが
///    残っていれば、何も変更せず `NeedsSizeDecision` を返す（復元点も作らない）
/// 2. 復元点を作る（不変条件 3）
/// 3. 利用者が「外す」と選んだファイルを保存対象から外す（`rm --cached` と .gitignore への追記）
/// 4. `add -A` → `commit`
pub(crate) fn save(
    runner: &GitRunner,
    repo: &Path,
    memo: &str,
    options: &SaveOptions,
    now: OffsetDateTime,
    meta: Meta,
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

    // 保存前のサイズ検査。ここまでは作業フォルダにもインデックスにも触れていない
    let findings = size_check::scan(runner, repo, options.limits)?;
    let exclude = size_check::validate_exclusions(&options.exclude, &findings)?;
    let pending = size_check::unresolved(&findings, &exclude, options.accept_warned);
    if !pending.is_empty() {
        return Ok(SaveOutcome::NeedsSizeDecision(pending));
    }

    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let restore_point_internal = meta.restore_point(runner, repo, &branch, "save", now)?;

    // 「外す」と選ばれたファイルを保存対象から外す。作業フォルダのファイルは消さない
    // （追跡解除は `rm --cached` のみ。未追跡ファイルには何もしない）
    if !exclude.is_empty() {
        for path in &exclude {
            let spec = format!(":(literal){path}");
            runner.run_ok(
                repo,
                &["rm", "--cached", "--ignore-unmatch", "-q", "--", &spec],
            )?;
        }
        size_check::append_ignore_patterns(repo, &exclude)?;
    }

    // add -A
    runner.run_ok(repo, &["add", "-A"])?;

    // 外したことで保存する内容が無くなった場合は、空の保存を作らない
    let staged = runner.run(repo, &["diff", "--cached", "--quiet", "--exit-code"])?;
    if staged.code == 0 {
        return Ok(SaveOutcome::NothingToSave);
    }

    // commit
    // メモの末尾に、どの PC で保存したかを残す（履歴の表示時に取り除く）
    let full_message = meta.message(memo);
    let _commit_output = runner.run_ok(
        repo,
        &["-c", "core.hooksPath=", "commit", "-m", &full_message],
    )?;

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
    limits: SizeLimits,
    meta: Meta,
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

    // 取り込み前の自動保存にも保存前と同じサイズ検査を行う（設計書 4.1 手順 1）。
    // 大きいファイルがあれば、何も変更せず（復元点も作らず）取り込みを見送る
    if has_unsaved {
        let findings = size_check::scan(runner, repo, limits)?;
        if !findings.is_empty() {
            return Ok(PullOutcome::NeedsSizeDecision(findings));
        }
    }

    // 作業フォルダ・履歴を変更する前に復元点を 1 回だけ作る（未保存の変更もここに含まれる）
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    let _ = meta.restore_point(runner, repo, &branch, "pull", now)?;

    // 未保存の変更があれば先に保存する
    if has_unsaved {
        runner.run_ok(repo, &["add", "-A"])?;
        let auto_message = meta.message(&labels.auto_save_memo);
        runner.run_ok(
            repo,
            &["-c", "core.hooksPath=", "commit", "-m", &auto_message],
        )?;
    }

    // 自動保存でアップロード待ちが増えうるため、改めて算出する
    let (ahead, behind) = ahead_behind(runner, repo)?;

    // 状態判定
    if behind == 0 {
        return Ok(PullOutcome::UpToDate);
    }

    // 取り込み前の HEAD（手順 8 で、取り込みで消えるファイルの内容を取り出す元）
    let pre_merge_head =
        String::from_utf8_lossy(&runner.run_ok(repo, &["rev-parse", "HEAD"])?.stdout)
            .trim()
            .to_string();

    if ahead == 0 {
        // fast-forward のみ
        runner.run_ok(repo, &["merge", "--ff-only", "@{u}"])?;
        restore_files_untracked_by_remote(runner, repo, &pre_merge_head);
        return Ok(PullOutcome::FastForwarded);
    }

    // 双方に差がある場合は backup ref を作成してから merge
    let _ = create_backup_ref(runner, repo, "pre-merge", now)?;

    // merge --no-edit @{u}（終了コード 0, 1 を許容）
    let merge_output = runner.run_allow_codes(repo, &["merge", "--no-edit", "@{u}"], &[0, 1])?;

    if merge_output.code == 0 {
        // merge 成功
        restore_files_untracked_by_remote(runner, repo, &pre_merge_head);
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

/// 取り込みで「相手側が保存対象から外した」ファイルが削除された場合に、取り込み前の内容を
/// 作業フォルダへ書き戻す（設計書 4.3 手順 8、4.8 の注意点への対策）。
///
/// - 対象は、取り込み前の HEAD にあり取り込み後の HEAD に無いパスのうち、取り込み後の
///   .gitignore で「保存しないファイル」になっていて、作業フォルダに実体が無いもの。
///   ignore されていない削除（相手が実際に消したファイル）は書き戻さない
/// - 書き戻しは `restore --source=<取り込み前> --worktree` の 1 ファイル指定のみ。インデックスは
///   触らない（取り込みの時点でインデックスからも外れている）。`rm` は使わない
/// - 既にファイルがあるパスは上書きしない（未追跡ファイルを消さない・壊さない。不変条件 6）
/// - 取り込み自体は成功しているため、個々の失敗は取り込みの失敗にしない。取り込み前の内容は
///   復元点（backup / snapshot）に残っている
fn restore_files_untracked_by_remote(runner: &GitRunner, repo: &Path, pre_merge_head: &str) {
    let Ok(removed) = runner.run_ok(
        repo,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=D",
            pre_merge_head,
            "HEAD",
        ],
    ) else {
        return;
    };
    for raw in removed.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let Some(path) = std::str::from_utf8(raw).ok() else {
            continue;
        };
        let Ok(rel) = crate::restore_file::normalize_project_path(path) else {
            continue;
        };
        if std::fs::symlink_metadata(repo.join(&rel)).is_ok() {
            continue;
        }
        let ignored = runner
            .run(repo, &["check-ignore", "-q", "--", &rel])
            .is_ok_and(|o| o.code == 0);
        if !ignored {
            continue;
        }
        let spec = format!(":(literal){rel}");
        let _ = runner.run(
            repo,
            &[
                "restore",
                "--source",
                pre_merge_head,
                "--worktree",
                "--",
                &spec,
            ],
        );
    }
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
        // この PC 側は HEAD、クラウド側は取り込み中の MERGE_HEAD
        let (this_saved_at, this_pc_name) = last_saved(runner, repo, "HEAD", &entry.path);
        let (cloud_saved_at, cloud_pc_name) = last_saved(runner, repo, "MERGE_HEAD", &entry.path);
        conflicts.push(ConflictFile {
            path: entry.path.clone(),
            kind,
            this_saved_at,
            cloud_saved_at,
            this_pc_name,
            cloud_pc_name,
        });
    }

    Ok(conflicts)
}

/// 指定の参照でそのパスを最後に変更したコミットの時刻（Unix 秒）と、その保存を作った PC の名前
/// （メモ末尾のトレーラー）を返す。
/// 参照が無い・履歴が空・解析できない場合は None（表示側で日時・PC 名を隠す）。
fn last_saved(
    runner: &GitRunner,
    repo: &Path,
    rev: &str,
    path: &str,
) -> (Option<i64>, Option<String>) {
    let Ok(out) = runner.run_ok(
        repo,
        &[
            "log",
            "--max-count=1",
            "--format=%ct%x1f%B",
            rev,
            "--",
            path,
        ],
    ) else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let (ct, message) = text.split_once('\u{1f}').unwrap_or((text.trim(), ""));
    let saved_at = ct.trim().parse().ok();
    let (_, pc_name) = split_trailer(message);
    (saved_at, pc_name)
}

/// `status --porcelain=v2 -z --branch` を実行して解析する
pub(crate) fn read_status(runner: &GitRunner, repo: &Path) -> Result<core_git::StatusV2, OpsError> {
    let out = runner.run_ok(repo, &["status", "--porcelain=v2", "-z", "--branch"])?;
    core_git::parse_status_v2(&out.stdout).map_err(|e| OpsError::Unexpected(e.to_string()))
}

/// 現在のブランチ名（detached HEAD では失敗する）
pub(crate) fn current_branch(runner: &GitRunner, repo: &Path) -> Result<String, OpsError> {
    let out = runner.run_ok(repo, &["symbolic-ref", "--short", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
pub(crate) fn parse_ls_tree_long(stdout: &[u8]) -> Vec<FileInHistory> {
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
    // 先頭が `-` の値はオプションとして解釈されるため受け付けない
    if [from, to]
        .iter()
        .any(|r| r.is_empty() || r.starts_with('-'))
    {
        return Err(OpsError::Unexpected("invalid commit".to_string()));
    }
    // 画面は「いま」の作業フォルダを `current` で指す。その場合は作業フォルダとの差分を取る
    let mut cmd = vec!["diff", "--unified=3", from];
    if to != "current" {
        cmd.push(to);
    }
    if let Some(p) = path {
        cmd.push("--");
        cmd.push(p);
    }

    let out = runner.run_ok(repo, &cmd)?;
    Ok(parse_unified_diff(&String::from_utf8_lossy(&out.stdout)))
}

/// unified diff の本文を行ごとの差分に変換する。
/// ファイルの見出し（`diff --git` から最初の `@@` まで）は読み飛ばし、`@@` の後は先頭 1 文字で
/// 種別を決める（内容が `--` や `++` で始まる行も取りこぼさない）。行番号は `@@` の値から振る。
pub(crate) fn parse_unified_diff(text: &str) -> Vec<DiffLine> {
    let mut lines = Vec::new();
    let mut in_hunk = false;
    let mut old_no: u32 = 0;
    let mut new_no: u32 = 0;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            in_hunk = false;
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            // "-<旧開始>[,<行数>] +<新開始>[,<行数>] @@ ..."
            let mut it = header.split(' ');
            let start = |s: Option<&str>, sign: char| {
                s.and_then(|v| v.strip_prefix(sign))
                    .and_then(|v| v.split(',').next())
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1)
            };
            old_no = start(it.next(), '-');
            new_no = start(it.next(), '+');
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }

        if let Some(content) = line.strip_prefix('-') {
            lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                line_number_old: Some(old_no),
                line_number_new: None,
                content: content.to_string(),
            });
            old_no += 1;
        } else if let Some(content) = line.strip_prefix('+') {
            lines.push(DiffLine {
                kind: DiffLineKind::Added,
                line_number_old: None,
                line_number_new: Some(new_no),
                content: content.to_string(),
            });
            new_no += 1;
        } else if let Some(content) = line.strip_prefix(' ') {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                line_number_old: Some(old_no),
                line_number_new: Some(new_no),
                content: content.to_string(),
            });
            old_no += 1;
            new_no += 1;
        }
        // "\ No newline at end of file" などは行として扱わない
    }

    lines
}

/// 元に戻す操作のプレビュー（影響ファイル一覧）。
pub(crate) fn restore_preview(
    runner: &GitRunner,
    repo: &Path,
    target_commit: &str,
) -> Result<RestorePreview, OpsError> {
    // 先頭が `-` の値はオプションとして解釈されるため受け付けない
    if target_commit.is_empty() || target_commit.starts_with('-') {
        return Err(OpsError::Unexpected("invalid commit".to_string()));
    }
    // 現在の状態と target_commit の差分を取得。`-z` でパスを引用符なしの生の値で受け取る
    // （日本語名・空白を含む名前も正しく扱える）。名前変更は削除と追加として扱う
    let diff_out = runner.run_ok(
        repo,
        &["diff", "--name-status", "-z", "--no-renames", target_commit],
    )?;

    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut created = Vec::new();

    // `<状態>\0<パス>\0` の繰り返し
    let mut tokens = diff_out
        .stdout
        .split(|b| *b == 0)
        .filter(|t| !t.is_empty())
        .map(|t| String::from_utf8_lossy(t).into_owned());
    while let (Some(status), Some(path)) = (tokens.next(), tokens.next()) {
        match status.as_str() {
            // 変更：サイズは 0 で固定（詳細は phase 2）
            "M" | "T" => modified.push(RestoreFileChange {
                path,
                size_from: 0,
                size_to: 0,
            }),
            // 追加（元に戻すと削除される）
            "A" => deleted.push(path),
            // 削除（元に戻すと復活する）
            "D" => created.push(path),
            _ => {}
        }
    }

    Ok(RestorePreview {
        modified,
        deleted,
        created,
    })
}

/// 指定の時点へ復元。復元前に復元点を作成し、指定時点のファイル状態に復元。
/// 戻り値は取り消し用の復元点（`refs/hikae/` 配下の ref 名。`undo_restore` に渡せる）。
pub(crate) fn restore(
    runner: &GitRunner,
    repo: &Path,
    target_commit: &str,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<Option<String>, OpsError> {
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

    // 未保存の変更があるかを復元点の作成前に調べておく（取り消し先の決定に使う）
    let dirty_before = !read_status(runner, repo)?.entries.is_empty();

    // 復元点を作成
    let point = meta.restore_point(runner, repo, &branch, "restore", now)?;

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

    crate::restore_file::undo_target(runner, repo, &branch, point, dirty_before)
}

/// 競合を解消
// 引数は呼び出し口（Ops::resolve）の引数に付帯情報（時刻・文言・署名）を足したもの
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve(
    runner: &GitRunner,
    repo: &Path,
    choices: &[(String, Choice)],
    keep_other_copy: bool,
    message: &str,
    now: OffsetDateTime,
    labels: &Labels,
    meta: Meta,
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
    let _ = meta.restore_point(runner, repo, &branch, "resolve", now)?;

    // 取り込み前の HEAD（競合中も HEAD は動かない）。解消後に、相手が保存対象から外したファイルを
    // 書き戻す元になる（設計書 4.3 手順 8）
    let pre_merge_head =
        String::from_utf8_lossy(&runner.run_ok(repo, &["rev-parse", "HEAD"])?.stdout)
            .trim()
            .to_string();

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

    // commit（メモの末尾に PC 名を残す）
    let full_message = meta.message(message);
    runner.run_ok(
        repo,
        &["-c", "core.hooksPath=", "commit", "-m", &full_message],
    )?;

    // 確認
    let remaining_conflicts = conflicts(runner, repo)?;
    if !remaining_conflicts.is_empty() {
        return Err(OpsError::Unexpected(
            "conflicts remain after resolution".to_string(),
        ));
    }

    // 競合で止まった取り込みにも、通常の取り込みと同じ手順 8 を行う。
    // 相手が追跡を外したファイルが取り込みで消えていれば、取り込み前の内容を書き戻す
    restore_files_untracked_by_remote(runner, repo, &pre_merge_head);

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
    meta: Meta,
) -> Result<(), OpsError> {
    // ブランチ名を取得
    let branch_output = runner.run(repo, &["symbolic-ref", "--short", "HEAD"])?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // 復元点を作成
    let _ = meta.restore_point(runner, repo, &branch, "abort-merge", now)?;

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
    limits: SizeLimits,
    meta: Meta,
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
        let pull_result = pull(runner, repo, now, labels, limits, meta)?;

        match &pull_result {
            PullOutcome::Conflicted { files } => {
                return Ok(UploadOutcome::NeedsResolve(files.clone()));
            }
            PullOutcome::NeedsSizeDecision(found) => {
                // 取り込みを見送ったので、アップロードもしない（何も変更していない）
                return Ok(UploadOutcome::NeedsSizeDecision(found.clone()));
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
