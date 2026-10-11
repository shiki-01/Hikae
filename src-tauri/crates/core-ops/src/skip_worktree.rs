// 「今後の変更だけ無視する」（skip-worktree、設計書 4.8）。
//
// 扱うのは git の印（インデックスの skip-worktree ビット）と、取り込みの前後の退避・書き戻し。
// 記録（この PC の SQLite）は app 層が持つ。ここでは記録を読まない。
//
// - 印の付け外し（set / clear）は印だけを変える。作業フォルダのファイルは変えない。変更の前に復元点を作る（不変条件 3）
// - 取り込み（pull）で相手が変更・削除するパスは、手元の内容を `.git/hikae-skip-stash/<ID>/` へバイト単位で退避する。
//   作業フォルダへ触るのは、退避が書けて読み戻して一致したあとだけ（不変条件 6）。そのあと印を外し、
//   作業フォルダを HEAD の版へ戻して merge する。成功したら退避の内容を書き戻し、印を付け直す（reapply）
// - 退避は復元点とは別の保険。復元点（スナップショット）には手元の内容が入る（一時インデックスで作るため）
// - `restore --staged` は、戻した先のインデックスの項目から印を外す（実機で確認）。復元系は `restore_keeping_skip` で印を付け直す
// - `add -A`（保存）と `git status` は、印の付いたファイルの変更と削除を無視する（実機で確認）
// - 手元で削除したまま無視しているファイルは、取り込みで相手の版が作業フォルダに戻る（実機で確認）。退避の対象外

use crate::models::{Labels, OpsError, SkipWorktreeReport};
use crate::open_path::{resolve_entry_in_project, OpenPathError};
use crate::operations::{conflicts, current_branch, entry_error, make_copy_path};
use crate::pc_name::Meta;
use crate::restore_file::normalize_project_path;
use core_git::GitRunner;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// 退避を置くフォルダの名前（`.git` の下に作る。作業フォルダにもリポジトリの内容にも出ない）
const STASH_DIR: &str = "hikae-skip-stash";
/// 退避の内容のファイル名
const STASH_CONTENT: &str = "content";
/// 退避の元のパスを書いた小さな JSON のファイル名
const STASH_META: &str = "meta.json";
/// 内容を比べるときに一度に読む大きさ
const COMPARE_BUF_BYTES: usize = 64 * 1024;

/// 取り込みの前に退避した 1 件
#[derive(Debug, Clone)]
struct StashEntry {
    /// 退避のフォルダ（`content` と `meta.json` が入る）
    dir: PathBuf,
    /// プロジェクトからの相対パス（`/` 区切り）
    path: String,
}

/// `ls-files -v -z` の出力を、(パス, 無視中か) の組にする（純関数）。
/// タグは先頭の 1 文字で、無視中（skip-worktree）は `S`。
pub(crate) fn parse_index_tags(stdout: &[u8]) -> Vec<(String, bool)> {
    stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|raw| {
            let text = std::str::from_utf8(raw).ok()?;
            let (tag, path) = text.split_once(' ')?;
            Some((path.to_string(), tag == "S"))
        })
        .collect()
}

/// 記録にあり、追跡中なのに無視が外れているパス（純関数）。昇順・重複なし。
pub(crate) fn drifted(recorded: &[String], tags: &[(String, bool)]) -> Vec<String> {
    let state: BTreeMap<&str, bool> = tags.iter().map(|(p, s)| (p.as_str(), *s)).collect();
    recorded
        .iter()
        .filter(|p| state.get(p.as_str()) == Some(&false))
        .cloned()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// インデックスの全パスと無視中かどうか（読み取りのみ）
fn index_tags(runner: &GitRunner, repo: &Path) -> Result<Vec<(String, bool)>, OpsError> {
    let out = runner.run_ok(repo, &["ls-files", "-v", "-z"])?;
    Ok(parse_index_tags(&out.stdout))
}

/// 無視中（skip-worktree）のパスを返す（読み取りのみ）。
pub(crate) fn list_skip_worktree(runner: &GitRunner, repo: &Path) -> Result<Vec<String>, OpsError> {
    Ok(index_tags(runner, repo)?
        .into_iter()
        .filter(|(_, skipped)| *skipped)
        .map(|(path, _)| path)
        .collect())
}

/// 記録にあり、追跡中で、無視が外れているパス（読み取りのみ）。
pub(crate) fn list_drift(
    runner: &GitRunner,
    repo: &Path,
    recorded: &[String],
) -> Result<Vec<String>, OpsError> {
    Ok(drifted(recorded, &index_tags(runner, repo)?))
}

/// 指定パスを正規化し、重複を除く。1 件も無ければ InvalidInput。
fn normalize_targets(paths: &[String]) -> Result<Vec<String>, OpsError> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for raw in paths {
        out.insert(normalize_project_path(raw)?);
    }
    if out.is_empty() {
        return Err(OpsError::InvalidInput("no files were given".to_string()));
    }
    Ok(out.into_iter().collect())
}

/// マージ競合が残っていれば拒否する（印の付け外しも、競合の解消を妨げないよう待つ）
fn refuse_during_conflict(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    let current = conflicts(runner, repo)?;
    if current.is_empty() {
        Ok(())
    } else {
        Err(OpsError::Conflict(current))
    }
}

/// `update-index <flag> -- <paths>`。パスは `--` の後のリテラル。
fn update_index(
    runner: &GitRunner,
    repo: &Path,
    flag: &str,
    paths: &[String],
) -> Result<(), OpsError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["update-index", flag, "--"];
    args.extend(paths.iter().map(String::as_str));
    runner.run_ok(repo, &args)?;
    Ok(())
}

/// 無視を指定する（印を付ける）。追跡中のファイルだけを受け付ける（それ以外は InvalidInput）。
/// 既に無視中のものは何もしない（その場合は復元点も作らない）。作業フォルダのファイルは変えない。
pub(crate) fn set_skip_worktree(
    runner: &GitRunner,
    repo: &Path,
    paths: &[String],
    now: OffsetDateTime,
    meta: Meta,
) -> Result<(), OpsError> {
    refuse_during_conflict(runner, repo)?;
    let targets = normalize_targets(paths)?;
    let tags: BTreeMap<String, bool> = index_tags(runner, repo)?.into_iter().collect();
    let mut to_set = Vec::new();
    for path in &targets {
        match tags.get(path) {
            None => {
                return Err(OpsError::InvalidInput(
                    "only tracked files can be ignored".to_string(),
                ))
            }
            Some(true) => {}
            Some(false) => to_set.push(path.clone()),
        }
    }
    if to_set.is_empty() {
        return Ok(());
    }
    // 退避が残っているファイルは、利用者の判断が済むまで印を付けない（古い内容を書き戻さないため）
    ensure_no_pending(&stash_root(runner, repo)?, &to_set)?;

    let branch = current_branch(runner, repo)?;
    let _ = meta.restore_point(runner, repo, &branch, "skip-worktree-set", now)?;
    update_index(runner, repo, "--skip-worktree", &to_set)
}

/// 無視を外す（印を外す）。無視中のファイルだけを対象にし、それ以外は何もしない。
/// 手元で変更していれば、以後は通常の未保存の変更として現れる。作業フォルダのファイルは変えない。
pub(crate) fn clear_skip_worktree(
    runner: &GitRunner,
    repo: &Path,
    paths: &[String],
    now: OffsetDateTime,
    meta: Meta,
) -> Result<(), OpsError> {
    refuse_during_conflict(runner, repo)?;
    let targets = normalize_targets(paths)?;
    let tags: BTreeMap<String, bool> = index_tags(runner, repo)?.into_iter().collect();
    let to_clear: Vec<String> = targets
        .into_iter()
        .filter(|path| tags.get(path) == Some(&true))
        .collect();
    if to_clear.is_empty() {
        return Ok(());
    }

    let branch = current_branch(runner, repo)?;
    let _ = meta.restore_point(runner, repo, &branch, "skip-worktree-clear", now)?;
    update_index(runner, repo, "--no-skip-worktree", &to_clear)
}

/// `restore --source <source> --staged --worktree -- <pathspec>` を行い、無視中の印を保つ（設計書 4.8）。
///
/// `restore --staged` は、戻した先のインデックスの項目から印を外す。そのため実行の前に無視中のパスを控え、
/// 実行の後に、無視中で HEAD にあるパスのインデックスを HEAD の版へ戻し、印を付け直す。
/// 作業フォルダの内容は戻した先のまま（復元の対象どおり）。HEAD に無い無視中のパスは印を付け直せない
/// （設計上、無視中のファイルは取り込まれた履歴を持つため、通常は起きない）。
pub(crate) fn restore_keeping_skip(
    runner: &GitRunner,
    repo: &Path,
    source: &str,
    pathspec: &str,
) -> Result<(), OpsError> {
    let skipped = list_skip_worktree(runner, repo)?;
    runner.run_ok(
        repo,
        &[
            "restore",
            "--source",
            source,
            "--staged",
            "--worktree",
            "--",
            pathspec,
        ],
    )?;
    repair_skip_bits(runner, repo, &skipped)
}

/// 無視中のパスのうち、インデックスにあり HEAD にもあるものについて、インデックスを HEAD の版へ戻し、
/// 印を付け直す。`restore --staged` は印の付いた項目を pathspec に一致させないため（実機で確認）、
/// 先に印を外してから戻す。
fn repair_skip_bits(runner: &GitRunner, repo: &Path, skipped: &[String]) -> Result<(), OpsError> {
    if skipped.is_empty() {
        return Ok(());
    }
    let indexed: BTreeSet<String> = index_tags(runner, repo)?
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let mut targets = Vec::new();
    for path in skipped {
        if indexed.contains(path) && in_head(runner, repo, path)? {
            targets.push(path.clone());
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    update_index(runner, repo, "--no-skip-worktree", &targets)?;
    let specs: Vec<String> = targets.iter().map(|p| format!(":(literal){p}")).collect();
    let mut args = vec!["restore", "--source", "HEAD", "--staged", "--"];
    args.extend(specs.iter().map(String::as_str));
    runner.run_ok(repo, &args)?;
    update_index(runner, repo, "--skip-worktree", &targets)
}

/// パスが HEAD の木にあるか（読み取りのみ）
fn in_head(runner: &GitRunner, repo: &Path, path: &str) -> Result<bool, OpsError> {
    let object = format!("HEAD:{path}");
    Ok(runner.run(repo, &["cat-file", "-e", &object])?.code == 0)
}

/// 退避を置くフォルダ（`.git/hikae-skip-stash`）
fn stash_root(runner: &GitRunner, repo: &Path) -> Result<PathBuf, OpsError> {
    let out = runner.run_ok(repo, &["rev-parse", "--git-dir"])?;
    let git_dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let base = if git_dir.is_absolute() {
        git_dir
    } else {
        repo.join(git_dir)
    };
    Ok(base.join(STASH_DIR))
}

/// 退避のフォルダ一覧（名前順）。フォルダが無ければ空。
fn stash_dirs(root: &Path) -> Result<Vec<PathBuf>, OpsError> {
    let read = match std::fs::read_dir(root) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut dirs = Vec::new();
    for entry in read {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// 退避フォルダのメタ情報から、元のパスを読む。壊れていれば None（そのフォルダには触らない）。
fn stash_path(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(STASH_META)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let raw = value.get("path")?.as_str()?;
    normalize_project_path(raw).ok()
}

/// 同じパスの退避が残っていれば拒否する（何も変えない）。
fn ensure_no_pending(root: &Path, paths: &[String]) -> Result<(), OpsError> {
    for dir in stash_dirs(root)? {
        if let Some(path) = stash_path(&dir) {
            if paths.contains(&path) {
                return Err(OpsError::SkipStashPending);
            }
        }
    }
    Ok(())
}

/// 2 つのファイルの内容が同じか（バイト単位。一定の大きさずつ読む）
fn files_equal(a: &Path, b: &Path) -> Result<bool, OpsError> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut fa = File::open(a)?;
    let mut fb = File::open(b)?;
    let mut buf_a = vec![0u8; COMPARE_BUF_BYTES];
    let mut buf_b = vec![0u8; COMPARE_BUF_BYTES];
    loop {
        let na = read_full(&mut fa, &mut buf_a)?;
        let nb = read_full(&mut fb, &mut buf_b)?;
        if na != nb || buf_a[..na] != buf_b[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// バッファを埋めるまで読む（末尾でだけ短くなる）
fn read_full(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// `src` を新規の `dst` へバイト単位で書き、書き終えてから読み戻して一致を確かめる。
/// 一致しなければ Err（`dst` は残しうるので、呼び出し側が消す）。
fn copy_verified(src: &Path, dst: &Path) -> Result<(), OpsError> {
    {
        let mut input = File::open(src)?;
        let mut output = File::create(dst)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    if files_equal(src, dst)? {
        Ok(())
    } else {
        Err(OpsError::Unexpected(
            "saved copy does not match the file".to_string(),
        ))
    }
}

/// 作業フォルダのパスの実体。無ければ `None`、通常のファイルなら `Some(絶対パス)`。
/// フォルダ・リンクなど通常のファイル以外は、退避も書き戻しもできないため拒否する。
fn worktree_file(repo: &Path, path: &str) -> Result<Option<PathBuf>, OpsError> {
    match resolve_entry_in_project(repo, path) {
        Ok(abs) => {
            if std::fs::symlink_metadata(&abs)?.is_file() {
                Ok(Some(abs))
            } else {
                Err(OpsError::InvalidInput(
                    "ignored file is not a regular file".to_string(),
                ))
            }
        }
        Err(OpenPathError::NotFound) => Ok(None),
        Err(e) => Err(entry_error(e)),
    }
}

/// 作業フォルダのファイルが、インデックス（HEAD の版）と同じ内容か（読み取りのみ）
fn matches_index(
    runner: &GitRunner,
    repo: &Path,
    path: &str,
    abs: &Path,
) -> Result<bool, OpsError> {
    let blob = runner.run_ok(repo, &["show", &format!(":{path}")])?;
    let local = std::fs::read(abs)?;
    Ok(blob.stdout == local)
}

/// 退避の内容を作業フォルダのファイルへ書き戻す。一時ファイルへ書いて読み戻して確かめ、そのあと差し替える。
fn write_back_verified(repo: &Path, path: &str, content: &Path) -> Result<(), OpsError> {
    let target = repo.join(path);
    let parent = target
        .parent()
        .ok_or_else(|| OpsError::InvalidInput("path has no parent folder".to_string()))?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".hikae-restore-{}", uuid::Uuid::new_v4().simple()));
    let result = copy_verified(content, &tmp)
        .and_then(|()| std::fs::rename(&tmp, &target).map_err(OpsError::from));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `HEAD..@{u}` で相手が変更・削除するパス（読み取りのみ）
fn incoming_paths(runner: &GitRunner, repo: &Path) -> Result<BTreeSet<String>, OpsError> {
    let out = runner.run_ok(
        repo,
        &["diff", "--name-only", "-z", "--no-renames", "HEAD", "@{u}"],
    )?;
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter_map(|raw| std::str::from_utf8(raw).ok())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// 退避の作成: 相手が変更・削除するパスの手元の内容を、作業フォルダへ触る前に退避する。
/// 途中で失敗したら、作った退避を消して Err を返す（作業フォルダ・インデックスは変えていない）。
fn stash_local_copies(
    root: &Path,
    repo: &Path,
    targets: &[String],
) -> Result<Vec<StashEntry>, OpsError> {
    let mut made: Vec<StashEntry> = Vec::new();
    for path in targets {
        match stash_one(root, repo, path) {
            Ok(entry) => made.push(entry),
            Err(e) => {
                for entry in &made {
                    let _ = std::fs::remove_dir_all(&entry.dir);
                }
                return Err(e);
            }
        }
    }
    Ok(made)
}

/// 1 件の退避を作る。内容を書いて読み戻して一致したあと、メタ情報を書く。
fn stash_one(root: &Path, repo: &Path, path: &str) -> Result<StashEntry, OpsError> {
    let abs = worktree_file(repo, path)?
        .ok_or_else(|| OpsError::Unexpected("nothing to save for the file".to_string()))?;
    let dir = root.join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&dir)?;
    let write = || -> Result<(), OpsError> {
        copy_verified(&abs, &dir.join(STASH_CONTENT))?;
        let meta = serde_json::json!({ "path": path }).to_string();
        std::fs::write(dir.join(STASH_META), meta)?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(StashEntry {
        dir,
        path: path.to_string(),
    })
}

/// 取り込み用に、印を外して作業フォルダを HEAD の版へ戻す（1 件）。
fn detach_one(runner: &GitRunner, repo: &Path, path: &str) -> Result<(), OpsError> {
    update_index(runner, repo, "--no-skip-worktree", &[path.to_string()])?;
    let spec = format!(":(literal){path}");
    runner.run_ok(
        repo,
        &["restore", "--source", "HEAD", "--worktree", "--", &spec],
    )?;
    Ok(())
}

/// 取り込みの準備（設計書 4.8、4.3 手順 8）。相手が変更・削除する無視中のパスについて、
/// 手元の内容を退避し、印を外して HEAD の版へ戻す。対象が無ければ何もしない。
///
/// 順序: 退避をすべて作って検証する（ここまでは作業フォルダに触らない）→ 1 件ずつ印を外して HEAD の版へ戻す。
/// 途中で失敗したら、それまでに変えたパスを元の印と内容へ戻し（退避の内容を書き戻す）、Err を返す。
/// 手元で削除したまま無視しているパスは対象外（取り込みで相手の版が作業フォルダに現れる）。
/// 同じパスの退避が残っていれば、何も変えずに Err を返す。
pub(crate) fn prepare_incoming(runner: &GitRunner, repo: &Path) -> Result<(), OpsError> {
    let skipped: BTreeSet<String> = index_tags(runner, repo)?
        .into_iter()
        .filter(|(_, skipped)| *skipped)
        .map(|(path, _)| path)
        .collect();
    if skipped.is_empty() {
        return Ok(());
    }
    let incoming = incoming_paths(runner, repo)?;
    let mut targets = Vec::new();
    for path in skipped.iter().filter(|p| incoming.contains(*p)) {
        if worktree_file(repo, path)?.is_some() {
            targets.push(path.clone());
        }
    }
    if targets.is_empty() {
        return Ok(());
    }

    let root = stash_root(runner, repo)?;
    ensure_no_pending(&root, &targets)?;
    let entries = stash_local_copies(&root, repo, &targets)?;

    for (i, entry) in entries.iter().enumerate() {
        if let Err(e) = detach_one(runner, repo, &entry.path) {
            // 変えたパスを元の印と内容へ戻す（退避の内容を書き戻す。書けなければ退避は残る）
            for done in &entries[..=i] {
                let _ = reapply_one(runner, repo, &done.dir, &done.path);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// 1 件の退避を書き戻して印を付け直す（設計書 4.8 の再適用の (1)）。
/// - 作業フォルダにファイルが無い、またはインデックス（HEAD の版）と同じ → 退避の内容を書き戻して印を付け、退避を消す
/// - 退避の内容と同じ → 印を付け、退避を消す（書き戻しは不要）
/// - それ以外（利用者がその間に編集した）→ 何も変えず、退避を残す（保留。`Ok(false)`）
fn reapply_one(runner: &GitRunner, repo: &Path, dir: &Path, path: &str) -> Result<bool, OpsError> {
    let content = dir.join(STASH_CONTENT);
    // 印を付けるとインデックスの内容は使われなくなる。手元の内容が復元点から入ってインデックスと
    // HEAD が食い違っていても、保存の対象にならないよう、先に HEAD の版へ揃える（作業フォルダは変えない）
    index_entry_to_head(runner, repo, path)?;
    let write_back = match worktree_file(repo, path)? {
        None => true,
        Some(abs) => {
            if files_equal(&content, &abs)? {
                false
            } else if matches_index(runner, repo, path, &abs)? {
                true
            } else {
                return Ok(false);
            }
        }
    };
    if write_back {
        write_back_verified(repo, path, &content)?;
    }
    update_index(runner, repo, "--skip-worktree", &[path.to_string()])?;
    let _ = std::fs::remove_dir_all(dir);
    Ok(true)
}

/// インデックスの項目を HEAD の版へ戻す（作業フォルダは変えない）。HEAD に無いパスは何もしない。
/// 印は外して戻す（呼び出し側が、必要なら付け直す）。
fn index_entry_to_head(runner: &GitRunner, repo: &Path, path: &str) -> Result<(), OpsError> {
    if !in_head(runner, repo, path)? {
        return Ok(());
    }
    update_index(runner, repo, "--no-skip-worktree", &[path.to_string()])?;
    let spec = format!(":(literal){path}");
    runner.run_ok(
        repo,
        &["restore", "--source", "HEAD", "--staged", "--", &spec],
    )?;
    Ok(())
}

/// マージ（取り込み）が途中で止まっているか（`MERGE_HEAD` の有無）
fn merge_in_progress(runner: &GitRunner, repo: &Path) -> Result<bool, OpsError> {
    let out = runner.run(repo, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])?;
    Ok(out.code == 0)
}

/// 退避の再適用（設計書 4.8）。取り込みの成功・解消・中止・起動時の復旧の後に呼ぶ。
///
/// - 競合で止まっている間（`MERGE_HEAD` がある間）は何もしない（退避は残る）
/// - 退避が無ければ git を呼ばない
/// - 相手が削除したパス（インデックスに無い）は書き戻さず、退避を残し、`removed_upstream` に入れる
/// - 作業フォルダの内容が判断できない（利用者の編集など）ものは、退避を残し、`pending` に入れる
/// - 退避フォルダが壊れている（元のパスが読めない）ものには触らない
pub(crate) fn reapply_skip_worktree(
    runner: &GitRunner,
    repo: &Path,
) -> Result<SkipWorktreeReport, OpsError> {
    let mut report = SkipWorktreeReport::default();
    let dirs = stash_dirs(&stash_root(runner, repo)?)?;
    if dirs.is_empty() || merge_in_progress(runner, repo)? {
        return Ok(report);
    }
    let tracked: BTreeSet<String> = index_tags(runner, repo)?
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    for dir in dirs {
        let Some(path) = stash_path(&dir) else {
            continue;
        };
        if !tracked.contains(&path) {
            report.removed_upstream.push(path);
            continue;
        }
        match reapply_one(runner, repo, &dir, &path) {
            Ok(true) => {}
            Ok(false) | Err(_) => report.pending.push(path),
        }
    }
    report.pending.sort();
    report.pending.dedup();
    report.removed_upstream.sort();
    report.removed_upstream.dedup();
    Ok(report)
}

/// 取り込みが失敗したあとの後始末（best-effort）。退避が残っていれば書き戻す。
pub(crate) fn recover_after_failed_incoming(runner: &GitRunner, repo: &Path) {
    let _ = reapply_skip_worktree(runner, repo);
}

/// 残っている退避の 1 件（設計書 4.8「退避に残っているファイル」）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipStashItem {
    /// 退避フォルダの名前（`.git/hikae-skip-stash` の直下。英数字・`-`・`_` のみ）
    pub id: String,
    /// 元のパス（プロジェクトからの相対パス、`/` 区切り）
    pub path: String,
    /// 退避している内容の大きさ（バイト）
    pub size_bytes: u64,
    /// 退避フォルダの更新時刻（Unix 秒）。取れなければ None
    pub saved_at: Option<i64>,
    /// 残っている理由（推定）
    pub reason: SkipStashReason,
}

/// 退避が残っている理由（推定。設計書 4.8）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipStashReason {
    /// 手元の内容を書き戻せなかった（インデックスにパスがあり、無視の印が無い。利用者がその間に編集した、など）
    Pending,
    /// 相手が削除したため書き戻さなかった（インデックスにパスが無い）
    DeletedUpstream,
    /// 判断できない（無視の印が付いたまま残っている、など）
    Unknown,
}

/// 退避の一覧と、壊れているため一覧に出さなかった件数
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkipStashList {
    /// 一覧（パス・フォルダ名の順）
    pub items: Vec<SkipStashItem>,
    /// 壊れている・不正なため一覧に出さなかった項目の数（退避のフォルダと、その中身の検査に失敗したもの）
    pub skipped_invalid: usize,
}

/// 別名で取り出した結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOutcome {
    /// 作成したコピーのパス（プロジェクトからの相対パス、`/` 区切り）
    pub copy_path: String,
}

/// 別名のファイルを置くときに、同名を避けて試す回数の上限（同時に同名が作られ続けた場合の打ち切り）
const COPY_NAME_ATTEMPTS: usize = 100;

/// 退避フォルダの名前として許すか（英数字・`-`・`_` のみ、1〜64 文字）。パスとして解釈させない。
fn valid_stash_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 退避フォルダの検査結果（読み取りのみで得たもの）
struct StashInfo {
    /// 元のパス（正規化済み）
    path: String,
    /// 内容の大きさ（バイト）
    size_bytes: u64,
    /// フォルダの更新時刻（Unix 秒）
    saved_at: Option<i64>,
}

/// フォルダの更新時刻を Unix 秒にする（取れなければ None）
fn modified_unix(meta: &std::fs::Metadata) -> Option<i64> {
    let secs = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(secs).ok()
}

/// 退避フォルダが一覧・取り出しに使えるか検査する（読み取りのみ）。
/// 条件: 名前が有効、フォルダ（リンクでない）、`meta.json` から元のパスが正規化できる、`content` が通常のファイル。
/// 1 つでも満たさなければ None（そのフォルダには触らない）。
fn inspect_stash(dir: &Path, name: &str) -> Option<StashInfo> {
    if !valid_stash_id(name) {
        return None;
    }
    let dir_meta = std::fs::symlink_metadata(dir).ok()?;
    if !dir_meta.is_dir() || dir_meta.file_type().is_symlink() {
        return None;
    }
    let content_meta = std::fs::symlink_metadata(dir.join(STASH_CONTENT)).ok()?;
    if !content_meta.is_file() {
        return None;
    }
    let path = stash_path(dir)?;
    Some(StashInfo {
        path,
        size_bytes: content_meta.len(),
        saved_at: modified_unix(&dir_meta),
    })
}

/// 残っている退避の一覧（読み取りのみ。設計書 4.8）。
/// 壊れた・不正なフォルダは一覧に出さず、`skipped_invalid` に数える。退避が無ければ git を呼ばない。
pub(crate) fn list_skip_stash(runner: &GitRunner, repo: &Path) -> Result<SkipStashList, OpsError> {
    let root = stash_root(runner, repo)?;
    let read = match std::fs::read_dir(&root) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SkipStashList::default()),
        Err(e) => return Err(e.into()),
    };
    let mut found: Vec<(String, StashInfo)> = Vec::new();
    let mut skipped_invalid = 0;
    for entry in read {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        match inspect_stash(&entry.path(), &name) {
            Some(info) => found.push((name, info)),
            None => skipped_invalid += 1,
        }
    }
    let tags: BTreeMap<String, bool> = if found.is_empty() {
        BTreeMap::new()
    } else {
        index_tags(runner, repo)?.into_iter().collect()
    };
    let mut items: Vec<SkipStashItem> = found
        .into_iter()
        .map(|(id, info)| SkipStashItem {
            reason: stash_reason(&tags, &info.path),
            id,
            path: info.path,
            size_bytes: info.size_bytes,
            saved_at: info.saved_at,
        })
        .collect();
    items.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.id.cmp(&b.id)));
    Ok(SkipStashList {
        items,
        skipped_invalid,
    })
}

/// 退避が残っている理由の推定（純関数）。`tags` はインデックスのパスと無視中かどうか。
fn stash_reason(tags: &BTreeMap<String, bool>, path: &str) -> SkipStashReason {
    match tags.get(path) {
        None => SkipStashReason::DeletedUpstream,
        Some(false) => SkipStashReason::Pending,
        Some(true) => SkipStashReason::Unknown,
    }
}

/// 退避フォルダの 1 件を指す絶対パス。id が不正・無い・リンクなら InvalidInput（何も変えない）
fn stash_dir_of(root: &Path, id: &str) -> Result<PathBuf, OpsError> {
    if !valid_stash_id(id) {
        return Err(OpsError::InvalidInput(
            "saved copy id is not valid".to_string(),
        ));
    }
    let dir = root.join(id);
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(dir),
        Ok(_) => Err(OpsError::InvalidInput(
            "saved copy is not a folder".to_string(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(OpsError::InvalidInput(
            "saved copy was not found".to_string(),
        )),
        Err(e) => Err(e.into()),
    }
}

/// 元のパスのフォルダ部分（`/` 区切り。プロジェクト直下なら空）
fn parent_rel(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

/// `rel_dir` のうち、作業フォルダに既にある最も深いフォルダと、まだ無い残りの成分を返す（読み取りのみ）。
/// 既存の成分にリンク・通常でないものがあれば、またはプロジェクトの外を指せば InvalidInput。
/// 作成するのは残りの成分だけ（新しく作るものはリンクにならないため、外へ書かない）。
fn existing_folder_chain(repo: &Path, rel_dir: &str) -> Result<(PathBuf, Vec<String>), OpsError> {
    let root_real = repo.canonicalize()?;
    let comps: Vec<&str> = rel_dir.split('/').filter(|c| !c.is_empty()).collect();
    let mut existing = repo.to_path_buf();
    let mut i = 0;
    while i < comps.len() {
        let next = existing.join(comps[i]);
        match std::fs::symlink_metadata(&next) {
            Ok(meta) => {
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err(OpsError::InvalidInput(
                        "folder of the file is not a plain folder".to_string(),
                    ));
                }
                existing = next;
                i += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    // 実体（リンク・ジャンクションをたどった先）がプロジェクトの配下か
    if !existing.canonicalize()?.starts_with(&root_real) {
        return Err(OpsError::InvalidInput(
            "folder of the file is outside the project".to_string(),
        ));
    }
    let missing = comps[i..].iter().map(|c| c.to_string()).collect();
    Ok((existing, missing))
}

/// 検証済みの一時ファイル `tmp`（内容は `content` と同じ）を、元のフォルダの別名のファイルとして置く。
/// 既存のファイルは上書きしない（硬いリンクは既存があれば失敗する。使えない FS では存在を確かめてから名前を変える）。
/// 置いたあと、内容が `content` と一致するかを読み戻して確かめる。失敗したら、置いたファイルを消す（自分で作ったもの）。
/// 戻り値は作ったコピーのプロジェクトからの相対パス。
fn place_copy(
    repo: &Path,
    path: &str,
    label: &str,
    now: OffsetDateTime,
    content: &Path,
    tmp: &Path,
) -> Result<String, OpsError> {
    copy_verified(content, tmp)?;
    for _ in 0..COPY_NAME_ATTEMPTS {
        let candidate = make_copy_path(repo, path, label, now);
        let target = repo.join(&candidate);
        match std::fs::hard_link(tmp, &target) {
            Ok(()) => {}
            // 同名が先に作られた。make_copy_path が次の番号を選ぶ
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // 硬いリンクが使えない FS。同名が無いことを確かめてから名前を変える
                if std::fs::symlink_metadata(&target).is_ok() {
                    continue;
                }
                std::fs::rename(tmp, &target)?;
            }
        }
        return match files_equal(content, &target) {
            Ok(true) => Ok(candidate),
            Ok(false) => {
                let _ = std::fs::remove_file(&target);
                Err(OpsError::Unexpected(
                    "saved copy does not match the file".to_string(),
                ))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&target);
                Err(e)
            }
        };
    }
    Err(OpsError::Unexpected(
        "no free name for the saved copy".to_string(),
    ))
}

/// 退避を、元のファイルと同じフォルダの別名のファイルとして取り出す（設計書 4.8）。
///
/// 順序: (1) 読み取りで検査（ここで拒否すれば何も変えない）→ (2) 復元点を作る（不変条件 3）→
/// (3) 別名のファイルを置いて内容を確かめる → (4) 確かめられたあとだけ退避フォルダを消す。
/// (3) で失敗したら退避は残し、置きかけのファイルは消す。作業フォルダの同名のファイルには触れない。
/// 退避を捨てる操作は持たない（取り出したコピーを消すかは利用者が決める）。
/// 競合の途中と、取り込みの途中（`MERGE_HEAD`）は拒否する（取り込みの後の再適用が退避を必要とするため）。
pub(crate) fn extract_skip_stash(
    runner: &GitRunner,
    repo: &Path,
    id: &str,
    now: OffsetDateTime,
    labels: &Labels,
    meta: Meta,
) -> Result<ExtractOutcome, OpsError> {
    refuse_during_conflict(runner, repo)?;
    if merge_in_progress(runner, repo)? {
        return Err(OpsError::Unexpected("a merge is in progress".to_string()));
    }
    let root = stash_root(runner, repo)?;
    let dir = stash_dir_of(&root, id)?;
    let info = inspect_stash(&dir, id)
        .ok_or_else(|| OpsError::InvalidInput("saved copy is broken".to_string()))?;
    let rel_dir = parent_rel(&info.path);
    // 読み取りの検査。外を指す・リンクを経由するなら、何も変えずに拒否する
    existing_folder_chain(repo, &rel_dir)?;

    let branch = current_branch(runner, repo)?;
    let _ = meta.restore_point(runner, repo, &branch, "skip-stash-extract", now)?;

    // 元のファイルのフォルダが無ければ作る（削除されたフォルダに、取り出した版を置くため）
    let (existing, missing) = existing_folder_chain(repo, &rel_dir)?;
    let mut folder = existing;
    for comp in &missing {
        folder = folder.join(comp);
    }
    std::fs::create_dir_all(&folder)?;

    let content = dir.join(STASH_CONTENT);
    // 一時ファイルは退避フォルダの中に作る（作業フォルダに残さないため）
    let tmp = dir.join(format!("extract-{}.tmp", uuid::Uuid::new_v4().simple()));
    let placed = place_copy(repo, &info.path, &labels.skip_stash, now, &content, &tmp);
    let _ = std::fs::remove_file(&tmp);
    let copy_path = placed?;

    // 置いて確かめたので、退避を消す。消せなくてもコピーは確かめ済みで、退避の内容はコピーに残っている
    let _ = std::fs::remove_dir_all(&dir);
    Ok(ExtractOutcome { copy_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_tags_mark_only_s_entries_as_skipped() {
        let raw = b"S dir/a b.psd\0H f.txt\0h g.txt\0";
        assert_eq!(
            parse_index_tags(raw),
            vec![
                ("dir/a b.psd".to_string(), true),
                ("f.txt".to_string(), false),
                ("g.txt".to_string(), false),
            ]
        );
        assert!(parse_index_tags(b"").is_empty());
    }

    #[test]
    fn drift_is_recorded_tracked_and_not_skipped() {
        let tags = vec![
            ("kept.psd".to_string(), true),
            ("off.psd".to_string(), false),
            ("sub/x.psd".to_string(), false),
        ];
        let recorded = vec![
            "kept.psd".to_string(),
            "off.psd".to_string(),
            "gone.psd".to_string(),
            "off.psd".to_string(),
        ];
        // 追跡されていない（gone.psd）と、印が付いているもの（kept.psd）は対象外。重複は除く
        assert_eq!(drifted(&recorded, &tags), vec!["off.psd".to_string()]);
        assert!(drifted(&[], &tags).is_empty());
    }

    #[test]
    fn files_equal_compares_bytes_across_chunks() {
        let tmp = tempfile::tempdir().expect("tmp");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        let big: Vec<u8> = (0..(COMPARE_BUF_BYTES * 2 + 17))
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(&a, &big).expect("write a");
        std::fs::write(&b, &big).expect("write b");
        assert!(files_equal(&a, &b).expect("equal"));

        let mut changed = big.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        std::fs::write(&b, &changed).expect("write b2");
        assert!(!files_equal(&a, &b).expect("differs at the end"));

        std::fs::write(&b, &big[..big.len() - 1]).expect("write b3");
        assert!(!files_equal(&a, &b).expect("shorter"));
    }

    #[test]
    fn stash_path_reads_only_a_valid_recorded_path() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().join("ok");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(STASH_META), r#"{"path":"docs/a.psd"}"#).expect("meta");
        assert_eq!(stash_path(&dir).as_deref(), Some("docs/a.psd"));

        // 元のパスが不正なもの、壊れた JSON は読まない（触らない）
        std::fs::write(dir.join(STASH_META), r#"{"path":"../escape"}"#).expect("meta");
        assert_eq!(stash_path(&dir), None);
        std::fs::write(dir.join(STASH_META), "{not json").expect("meta");
        assert_eq!(stash_path(&dir), None);
    }

    #[test]
    fn stash_id_must_be_a_plain_name() {
        assert!(valid_stash_id("pending-1"));
        assert!(valid_stash_id("0123456789abcdef0123456789abcdef"));
        let long = "x".repeat(65);
        for bad in ["", "../x", "a/b", "a b", ".hidden", "a.b", long.as_str()] {
            assert!(!valid_stash_id(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn stash_reason_follows_the_index() {
        let tags: BTreeMap<String, bool> = [
            ("kept.psd".to_string(), false),
            ("ignored.psd".to_string(), true),
        ]
        .into_iter()
        .collect();
        assert_eq!(stash_reason(&tags, "kept.psd"), SkipStashReason::Pending);
        assert_eq!(stash_reason(&tags, "ignored.psd"), SkipStashReason::Unknown);
        assert_eq!(
            stash_reason(&tags, "gone.psd"),
            SkipStashReason::DeletedUpstream
        );
    }
}
