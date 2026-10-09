// 外部のファイル・フォルダをプロジェクトへコピーする（設計書 4.7、ドラッグ＆ドロップ）。
//
// - コピーのみ（元ファイルは動かさない）。保存（commit）は別操作
// - 同名があるとき（`AddConflictPolicy`）:
//   - `Ask`: 何も書かずに「確認が必要な同名のファイル」の一覧を返す。画面が選択を添えてやり直す
//   - `KeepBoth`: `名前 (2).ext` のように別名にして両方残す（`create_new` で存在確認と作成を同時に行う）
//   - `Replace`: 置き換える。上書きは既存ファイルを壊す操作なので、次の安全策をすべて満たしたときだけ行い、
//     満たせなければ上書きせずに別名にする（`replace_refused`）
//     (a) 上書きの前に復元点（既存ファイルの内容を含む）を作り、その中の内容が、いまのファイルと
//         バイト単位で一致することを確かめる（`discard_new_file` と同じ流儀）。追跡済みで未変更のファイルは
//         HEAD の内容でも確かめる。保存対象外のファイルは復元点に入らないため、置き換えない
//     (b) 復元点に控えを残せない大きさ（警告閾値を超える）のファイル、通常のファイルではないもの
//         （フォルダ・リンク）は置き換えない
//     (c) 書き込みは同じフォルダの一時ファイルへ行い、直前にもう一度ファイルが変わっていないことを
//         確かめてから `rename` で差し替える（書き込みの途中で失敗しても、既存のファイルは壊れない）
// - フォルダは中のファイルを再帰的にコピーする。上限（ファイル 1,000 件・階層 20）を超えるときは、
//   何もコピーせずに断る。リンク（symlink / junction）はたどらず、隠しファイル・OS の一時ファイルは
//   飛ばし、結果に「飛ばしたもの」として報告する
// - 100MB 超のファイルは追加しない（E07）。警告閾値（設定 `large_file_warn_mb`）超は追加して
//   `large` で知らせる（E08）。サイズ検査はフォルダの中のファイルも 1 件ずつ行う
// - 作業フォルダを変更するため、コピー前に復元点を作る（不変条件 3）。コピー中に失敗したファイルは、
//   自分が `create_new` で作った途中のファイルだけを片付ける（既存のファイルには触れない）

use crate::models::*;
use crate::open_path::{resolve_in_project, OpenPathError};
use crate::operations::{current_branch, read_status};
use crate::pc_name::Meta;
use crate::restore_file::{normalize_project_path, undo_target};
use crate::size_check::SizeLimits;
use core_git::GitRunner;
use core_safety::list_snapshots;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// この大きさを超えるファイルは警告する（設計書 7章の初期値 50MB。ちょうどは警告しない）
pub const LARGE_FILE_WARN_BYTES: u64 = 50 * 1024 * 1024;
/// この大きさを超えるファイルは追加しない（GitHub の上限 100MB）
pub const LARGE_FILE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;
/// フォルダのドロップで受け付けるファイル数の上限
pub const FOLDER_MAX_FILES: usize = 1000;
/// フォルダのドロップで受け付ける階層の上限（ドロップしたフォルダの中の階層）
pub const FOLDER_MAX_DEPTH: usize = 20;

/// 別名の連番の上限（これ以上は異常とみなして失敗させる）
const MAX_RENAME_ATTEMPTS: u32 = 1000;

/// ファイル追加の要求
#[derive(Debug, Clone)]
pub struct AddFilesRequest {
    /// コピー元（ファイルまたはフォルダ）
    pub sources: Vec<PathBuf>,
    /// 追加先のフォルダ（プロジェクトからの相対パス）。空ならプロジェクト直下
    pub dest_subdir: String,
    /// 同名があるときの方針
    pub on_conflict: AddConflictPolicy,
    /// 追加先のパスごとの選択（方針より優先する）
    pub decisions: Vec<AddConflictDecision>,
    /// サイズの閾値（警告は設定 `large_file_warn_mb`、保存不可は 100MB 固定）
    pub limits: SizeLimits,
}

/// 追加先のフォルダを検証して絶対パスにする。空文字列はプロジェクト直下。
/// 返す文字列はプロジェクトからの相対パス（直下は空）。
fn resolve_dest_dir(repo: &Path, dest_subdir: &str) -> Result<(PathBuf, String), OpsError> {
    if dest_subdir.is_empty() {
        let root = repo
            .canonicalize()
            .map_err(|e| OpsError::InvalidInput(format!("project folder is not available: {e}")))?;
        return Ok((root, String::new()));
    }
    let rel = normalize_project_path(dest_subdir)?;
    let dir = resolve_in_project(repo, &rel).map_err(|e| match e {
        OpenPathError::NotFound => OpsError::InvalidInput("destination folder not found".into()),
        other => OpsError::InvalidInput(format!("destination is not allowed: {other}")),
    })?;
    if !dir.is_dir() {
        return Err(OpsError::InvalidInput(
            "destination is not a folder".to_string(),
        ));
    }
    Ok((dir, rel))
}

/// 衝突しないファイル名の候補。n=1 は元の名前、n>=2 は `名前 (n).ext`
fn candidate_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_string();
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    match p.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem} ({n}).{ext}"),
        None => format!("{stem} ({n})"),
    }
}

/// 上書きせずにコピーする。`create_new` で作成と存在確認を同時に行い、競合しても既存を壊さない。
/// 成功したら使った名前と、別名にしたかを返す。
fn copy_without_overwrite(src: &Path, dest_dir: &Path, name: &str) -> io::Result<(String, bool)> {
    // 読めないコピー元で空ファイルを作らないよう、先にコピー元を開く
    let mut input = File::open(src)?;
    for n in 1..=MAX_RENAME_ATTEMPTS {
        let candidate = candidate_name(name, n);
        let target = dest_dir.join(&candidate);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut out) => {
                return match io::copy(&mut input, &mut out) {
                    Ok(_) => Ok((candidate, n > 1)),
                    Err(e) => {
                        // いま自分が作った途中のコピーだけを片付ける（既存ファイルには触れない）
                        drop(out);
                        let _ = std::fs::remove_file(&target);
                        Err(e)
                    }
                };
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "too many files with the same name",
    ))
}

/// 追加するファイル 1 件の計画
struct Item {
    src: PathBuf,
    /// 追加先のフォルダからの成分（最後がファイル名）
    comps: Vec<String>,
    /// 結果に出す元の名前（ドロップしたものからの相対パス）
    label: String,
    size: u64,
}

/// コピー元の走査結果
#[derive(Default)]
struct Scan {
    items: Vec<Item>,
    rejected: Vec<RejectedFile>,
    skipped: Vec<SkippedItem>,
}

/// 名前が OS・Office の一時ファイルか、隠しファイルか
fn skip_reason(name: &str, meta: &std::fs::Metadata) -> Option<SkipReason> {
    let lower = name.to_lowercase();
    if lower == ".ds_store"
        || lower == "thumbs.db"
        || lower == "desktop.ini"
        || name.starts_with("~$")
        || (lower.starts_with("~wrl") && lower.ends_with(".tmp"))
    {
        return Some(SkipReason::OsTemp);
    }
    if name.starts_with('.') || is_hidden_attribute(meta) {
        return Some(SkipReason::Hidden);
    }
    None
}

#[cfg(windows)]
fn is_hidden_attribute(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn is_hidden_attribute(_meta: &std::fs::Metadata) -> bool {
    false
}

/// ドロップしたフォルダの追加先の名前。同名の「フォルダ」があれば中身をまとめ、同名の
/// ファイル・リンクがあれば `名前 (2)` のように別の名前にする（フォルダでファイルを上書きしない）。
fn pick_folder_name(dest_dir: &Path, name: &str) -> String {
    for n in 1..=MAX_RENAME_ATTEMPTS {
        let candidate = if n <= 1 {
            name.to_string()
        } else {
            format!("{name} ({n})")
        };
        match std::fs::symlink_metadata(dest_dir.join(&candidate)) {
            Err(e) if e.kind() == ErrorKind::NotFound => return candidate,
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => return candidate,
            _ => continue,
        }
    }
    name.to_string()
}

/// フォルダを再帰的に走査する状態
struct Walker<'a> {
    limits: SizeLimits,
    files: usize,
    scan: &'a mut Scan,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        dir: &Path,
        comps: &mut Vec<String>,
        labels: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), OpsError> {
        let mut entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(Result::ok).collect(),
            Err(_) => {
                self.scan.rejected.push(RejectedFile {
                    name: labels.join("/"),
                    reason: AddRejectReason::Unreadable,
                });
                return Ok(());
            }
        };
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            labels.push(name.clone());
            let label = labels.join("/");
            let step = (|| -> Result<(), OpsError> {
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    self.scan.rejected.push(RejectedFile {
                        name: label.clone(),
                        reason: AddRejectReason::Unreadable,
                    });
                    return Ok(());
                };
                // リンクはたどらない
                if meta.file_type().is_symlink() {
                    self.scan.skipped.push(SkippedItem {
                        name: label.clone(),
                        reason: SkipReason::Link,
                    });
                    return Ok(());
                }
                if let Some(reason) = skip_reason(&name, &meta) {
                    self.scan.skipped.push(SkippedItem {
                        name: label.clone(),
                        reason,
                    });
                    return Ok(());
                }
                if meta.is_dir() {
                    if depth + 1 > FOLDER_MAX_DEPTH {
                        return Err(OpsError::AddRefused(AddRefusal::TooDeep {
                            limit: FOLDER_MAX_DEPTH as u32,
                        }));
                    }
                    comps.push(name.clone());
                    let result = self.walk(&path, comps, labels, depth + 1);
                    comps.pop();
                    return result;
                }
                if meta.is_file() {
                    self.files += 1;
                    if self.files > FOLDER_MAX_FILES {
                        return Err(OpsError::AddRefused(AddRefusal::TooManyFiles {
                            limit: FOLDER_MAX_FILES as u32,
                        }));
                    }
                    if meta.len() > self.limits.block_bytes {
                        self.scan.rejected.push(RejectedFile {
                            name: label.clone(),
                            reason: AddRejectReason::TooLarge { size: meta.len() },
                        });
                    } else {
                        let mut item_comps = comps.clone();
                        item_comps.push(name.clone());
                        self.scan.items.push(Item {
                            src: path.clone(),
                            comps: item_comps,
                            label: label.clone(),
                            size: meta.len(),
                        });
                    }
                    return Ok(());
                }
                self.scan.rejected.push(RejectedFile {
                    name: label.clone(),
                    reason: AddRejectReason::NotAFile,
                });
                Ok(())
            })();
            labels.pop();
            step?;
        }
        Ok(())
    }
}

/// コピー元を調べて、コピーできるものと断るものに分ける（何も変更しない）。
fn scan_sources(
    sources: &[PathBuf],
    limits: SizeLimits,
    dest_dir: &Path,
) -> Result<Scan, OpsError> {
    let mut scan = Scan::default();
    let mut files = 0usize;
    for src in sources {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let reject = |reason| RejectedFile {
            name: name.clone(),
            reason,
        };
        let meta = match std::fs::symlink_metadata(src) {
            Ok(m) => m,
            Err(_) => {
                scan.rejected.push(reject(AddRejectReason::Unreadable));
                continue;
            }
        };
        // リンクはたどらない
        if meta.file_type().is_symlink() {
            scan.skipped.push(SkippedItem {
                name: name.clone(),
                reason: SkipReason::Link,
            });
        } else if name.is_empty() {
            scan.rejected.push(reject(AddRejectReason::NotAFile));
        } else if meta.is_file() {
            files += 1;
            if files > FOLDER_MAX_FILES {
                return Err(OpsError::AddRefused(AddRefusal::TooManyFiles {
                    limit: FOLDER_MAX_FILES as u32,
                }));
            }
            if meta.len() > limits.block_bytes {
                scan.rejected
                    .push(reject(AddRejectReason::TooLarge { size: meta.len() }));
            } else {
                scan.items.push(Item {
                    src: src.clone(),
                    comps: vec![name.clone()],
                    label: name.clone(),
                    size: meta.len(),
                });
            }
        } else if meta.is_dir() {
            let top = pick_folder_name(dest_dir, &name);
            let mut walker = Walker {
                limits,
                files,
                scan: &mut scan,
            };
            let mut comps = vec![top];
            let mut labels = vec![name.clone()];
            walker.walk(src, &mut comps, &mut labels, 0)?;
            files = walker.files;
        } else {
            scan.rejected.push(reject(AddRejectReason::NotAFile));
        }
    }
    Ok(scan)
}

/// 追加先にすでにあるもの
enum Existing {
    Nothing,
    /// 通常のファイル（大きさ）
    File(u64),
    /// フォルダ・リンクなど、置き換えられないもの
    Other,
    /// 調べられない
    Unknown,
}

fn existing_at(path: &Path) -> Existing {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Existing::Other,
        Ok(m) if m.is_file() => Existing::File(m.len()),
        Ok(_) => Existing::Other,
        Err(e) if e.kind() == ErrorKind::NotFound => Existing::Nothing,
        Err(_) => Existing::Unknown,
    }
}

/// 計画した 1 件の書き方
enum Mode {
    /// 追加先に何も無い。上書きしない書き方で作る
    Plain,
    /// 別名にして両方残す（`refused` は、置き換えを選ばれたが安全に置き換えられなかった場合）
    KeepBoth { refused: bool },
    /// 置き換える
    Replace,
}

struct Job {
    item: Item,
    rel: String,
    mode: Mode,
}

/// プロジェクトからの相対パス（`/` 区切り）にする
fn rel_path(dest_rel: &str, comps: &[String]) -> String {
    let joined = comps.join("/");
    if dest_rel.is_empty() {
        joined
    } else {
        format!("{dest_rel}/{joined}")
    }
}

/// 追加先のフォルダの連なりを用意する。無ければ作り、あれば本物のフォルダ（リンクではない）であること、
/// 実体がプロジェクトの配下であることを確かめる。作ったフォルダは `created` に積む。
fn ensure_dirs(
    root_real: &Path,
    dest_dir: &Path,
    comps: &[String],
    created: &mut Vec<PathBuf>,
) -> io::Result<PathBuf> {
    let mut cur = dest_dir.to_path_buf();
    for comp in comps {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    ErrorKind::AlreadyExists,
                    "a file or link is in the way",
                ))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                std::fs::create_dir(&cur)?;
                created.push(cur.clone());
            }
            Err(e) => return Err(e),
        }
    }
    let real = cur.canonicalize()?;
    if !real.starts_with(root_real) {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "destination is outside the project",
        ));
    }
    Ok(real)
}

/// 復元点の中の `rel` が、いまのファイルの内容とバイト単位で一致するか
fn stored_equals(runner: &GitRunner, repo: &Path, commit: &str, rel: &str, current: &[u8]) -> bool {
    match runner.run(repo, &["cat-file", "-p", &format!("{commit}:{rel}")]) {
        Ok(out) => out.code == 0 && out.stdout == current,
        Err(_) => false,
    }
}

/// 置き換えの結果
enum Replaced {
    Done,
    /// 安全に置き換えられなかった（上書きしていない）。別名にする
    Refused,
    /// 置き換え先が他のアプリで使用中
    InUse,
    Failed,
}

/// 一時ファイルへ書いてから差し替える。既存ファイルの内容が復元点にあることを確かめ、
/// 直前にもう一度、変わっていないことを確かめる。
#[allow(clippy::too_many_arguments)]
fn replace_file(
    runner: &GitRunner,
    repo: &Path,
    backups: &[String],
    src: &Path,
    parent: &Path,
    name: &str,
    rel: &str,
    limits: SizeLimits,
) -> Replaced {
    let target = parent.join(name);
    let Ok(before) = std::fs::symlink_metadata(&target) else {
        return Replaced::Refused;
    };
    if !before.is_file() || before.file_type().is_symlink() || before.len() > limits.warn_bytes {
        return Replaced::Refused;
    }
    let Ok(current) = std::fs::read(&target) else {
        return Replaced::Refused;
    };
    // (a) 復元点に、いまの内容があること
    if !backups
        .iter()
        .any(|commit| stored_equals(runner, repo, commit, rel, &current))
    {
        return Replaced::Refused;
    }

    // (c) 一時ファイルへ書く（自分が create_new で作ったものだけを片付ける）
    let tmp = parent.join(format!(".hikae-tmp-{}", uuid::Uuid::new_v4().simple()));
    let written = (|| -> io::Result<()> {
        let mut input = File::open(src)?;
        let mut out = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        io::copy(&mut input, &mut out)?;
        out.sync_all()
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Replaced::Failed;
    }

    // 直前の再確認。読んだあとにファイルが変わっていたら、新しい内容を上書きしない
    let unchanged = std::fs::symlink_metadata(&target).is_ok_and(|after| {
        after.is_file()
            && after.len() == before.len()
            && after.modified().ok() == before.modified().ok()
    }) && std::fs::read(&target).is_ok_and(|now| now == current);
    if !unchanged {
        let _ = std::fs::remove_file(&tmp);
        return Replaced::Refused;
    }

    match std::fs::rename(&tmp, &target) {
        Ok(()) => Replaced::Done,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            if crate::file_in_use::is_sharing_violation(&e) {
                Replaced::InUse
            } else {
                Replaced::Failed
            }
        }
    }
}

/// 外部のファイル・フォルダをプロジェクト配下へコピーする。
pub(crate) fn add_files(
    runner: &GitRunner,
    repo: &Path,
    request: &AddFilesRequest,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<AddFilesOutcome, OpsError> {
    let (dest_dir, dest_rel) = resolve_dest_dir(repo, &request.dest_subdir)?;
    let root_real = repo
        .canonicalize()
        .map_err(|e| OpsError::InvalidInput(format!("project folder is not available: {e}")))?;
    let limits = request.limits;

    // 計画: 何も変更せず、コピーできるものと断るものに分ける
    let scan = scan_sources(&request.sources, limits, &dest_dir)?;
    let mut outcome = AddFilesOutcome {
        rejected: scan.rejected,
        skipped: scan.skipped,
        ..AddFilesOutcome::default()
    };

    let decisions: HashMap<&str, AddConflictAction> = request
        .decisions
        .iter()
        .map(|d| (d.path.as_str(), d.action))
        .collect();
    let mut claimed: HashSet<String> = HashSet::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut asks: Vec<NameConflict> = Vec::new();
    for item in scan.items {
        let rel = rel_path(&dest_rel, &item.comps);
        // 同じ追加先を先に取った別のファイルがある（コピー元どうしの同名）
        let shadowed = !claimed.insert(rel.to_lowercase());
        let target = item
            .comps
            .iter()
            .fold(dest_dir.clone(), |acc, c| acc.join(c));
        let existing = existing_at(&target);
        let mode = match existing {
            Existing::Unknown => {
                outcome.rejected.push(RejectedFile {
                    name: item.label.clone(),
                    reason: AddRejectReason::Unreadable,
                });
                continue;
            }
            _ if shadowed => Mode::KeepBoth { refused: false },
            Existing::Nothing => Mode::Plain,
            Existing::File(_) | Existing::Other => {
                let can_replace =
                    matches!(existing, Existing::File(len) if len <= limits.warn_bytes);
                let action = decisions
                    .get(rel.as_str())
                    .copied()
                    .or(match request.on_conflict {
                        AddConflictPolicy::Ask => None,
                        AddConflictPolicy::KeepBoth => Some(AddConflictAction::KeepBoth),
                        AddConflictPolicy::Replace => Some(AddConflictAction::Replace),
                    });
                match action {
                    None => {
                        asks.push(NameConflict {
                            path: rel,
                            can_replace,
                        });
                        continue;
                    }
                    Some(AddConflictAction::Skip) => continue,
                    Some(AddConflictAction::KeepBoth) => Mode::KeepBoth { refused: false },
                    Some(AddConflictAction::Replace) if can_replace => Mode::Replace,
                    Some(AddConflictAction::Replace) => Mode::KeepBoth { refused: true },
                }
            }
        };
        jobs.push(Job { item, rel, mode });
    }

    // 確認が必要な同名がある間は、何も書かない
    if !asks.is_empty() {
        return Ok(AddFilesOutcome {
            needs_decision: asks,
            ..AddFilesOutcome::default()
        });
    }
    if jobs.is_empty() {
        return Ok(outcome);
    }

    // 作業フォルダを変更する前に復元点を作る（不変条件 3）
    let branch = current_branch(runner, repo)?;
    let dirty_before = !read_status(runner, repo)?.entries.is_empty();
    let point = meta.restore_point(runner, repo, &branch, "add-files", now)?;

    // 置き換える前に内容を照合する復元点の候補（作った復元点、HEAD、直近の自動保存）
    let mut backups: Vec<String> = Vec::new();
    if jobs.iter().any(|j| matches!(j.mode, Mode::Replace)) {
        if let Some(s) = &point.snapshot {
            backups.push(s.commit.clone());
        }
        if let Ok(out) = runner.run(repo, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]) {
            let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.code == 0 && !head.is_empty() {
                backups.push(head);
            }
        }
        if let Some(latest) = list_snapshots(runner, repo, &branch)?.into_iter().next() {
            backups.push(latest.commit);
        }
    }

    let mut created_dirs: Vec<PathBuf> = Vec::new();
    let mut any_replaced = false;
    for job in jobs {
        let Job { item, rel, mode } = job;
        let name = item.comps.last().cloned().unwrap_or_default();
        let parent_comps = &item.comps[..item.comps.len().saturating_sub(1)];
        let parent = match ensure_dirs(&root_real, &dest_dir, parent_comps, &mut created_dirs) {
            Ok(p) => p,
            Err(e) if crate::disk_full::is_disk_full_error(&e) => {
                for dir in created_dirs.iter().rev() {
                    let _ = std::fs::remove_dir(dir);
                }
                return Err(OpsError::DiskFull);
            }
            Err(_) => {
                outcome.rejected.push(RejectedFile {
                    name: item.label,
                    reason: AddRejectReason::Unreadable,
                });
                continue;
            }
        };
        let large = item.size > limits.warn_bytes;

        // 置き換え。安全に置き換えられなければ、別名にして両方残す
        let mut refused = matches!(mode, Mode::KeepBoth { refused: true });
        if matches!(mode, Mode::Replace) {
            match replace_file(
                runner, repo, &backups, &item.src, &parent, &name, &rel, limits,
            ) {
                Replaced::Done => {
                    any_replaced = true;
                    outcome.added.push(AddedFile {
                        path: rel,
                        renamed: false,
                        large,
                        replaced: true,
                        replace_refused: false,
                    });
                    continue;
                }
                Replaced::Refused => refused = true,
                Replaced::InUse => {
                    outcome.rejected.push(RejectedFile {
                        name: item.label,
                        reason: AddRejectReason::InUse,
                    });
                    continue;
                }
                Replaced::Failed => {
                    outcome.rejected.push(RejectedFile {
                        name: item.label,
                        reason: AddRejectReason::Unreadable,
                    });
                    continue;
                }
            }
        }

        match copy_without_overwrite(&item.src, &parent, &name) {
            Ok((final_name, renamed)) => {
                let path = if let Some((dir, _)) = rel.rsplit_once('/') {
                    format!("{dir}/{final_name}")
                } else {
                    final_name
                };
                outcome.added.push(AddedFile {
                    path,
                    renamed,
                    large,
                    replaced: false,
                    replace_refused: refused,
                });
            }
            // 空き容量が足りないと以降のコピーもすべて失敗するため、ここで止めて知らせる（E13）。
            // コピー済みのファイルはそのまま残る（途中のコピーは片付け済み）
            Err(e) if crate::disk_full::is_disk_full_error(&e) => {
                for dir in created_dirs.iter().rev() {
                    let _ = std::fs::remove_dir(dir);
                }
                return Err(OpsError::DiskFull);
            }
            Err(_) => outcome.rejected.push(RejectedFile {
                name: item.label,
                reason: AddRejectReason::Unreadable,
            }),
        }
    }

    // 失敗して空のままになった、自分が作ったフォルダを片付ける（中身があれば消えない）
    for dir in created_dirs.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }

    if any_replaced {
        outcome.undo_ref = undo_target(runner, repo, &branch, point, dirty_before)?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_name_numbers_before_extension() {
        assert_eq!(candidate_name("a.txt", 1), "a.txt");
        assert_eq!(candidate_name("a.txt", 2), "a (2).txt");
        assert_eq!(candidate_name("a.tar.gz", 3), "a.tar (3).gz");
        assert_eq!(candidate_name("README", 2), "README (2)");
        assert_eq!(candidate_name("報告 書.docx", 2), "報告 書 (2).docx");
    }

    #[test]
    fn relative_path_joins_with_slash() {
        assert_eq!(rel_path("", &["a".into(), "b.txt".into()]), "a/b.txt");
        assert_eq!(rel_path("docs", &["b.txt".into()]), "docs/b.txt");
    }
}
