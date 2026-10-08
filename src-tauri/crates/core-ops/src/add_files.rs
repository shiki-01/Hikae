// 外部のファイルをプロジェクトへコピーする（設計書 4.7、ドラッグ＆ドロップ）。
//
// - コピーのみ（元ファイルは動かさない）。保存（commit）は別操作
// - 既存のファイルは上書きしない。同名があれば「名前 (2).ext」のように別名にする
// - 100MB 超は追加しない（E07）。50MB 以上は追加するが `large` で知らせる（E08）
// - 作業フォルダを変更するため、コピー前に復元点を作る（不変条件 3）

use crate::models::*;
use crate::open_path::{resolve_in_project, OpenPathError};
use crate::operations::current_branch;
use crate::restore_file::normalize_project_path;
use core_git::GitRunner;
use core_safety::create_restore_point;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// この大きさ以上のファイルは警告する（設計書 7章の初期値 50MB）
pub const LARGE_FILE_WARN_BYTES: u64 = 50 * 1024 * 1024;
/// この大きさを超えるファイルは追加しない（GitHub の上限 100MB）
pub const LARGE_FILE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

/// 別名の連番の上限（これ以上は異常とみなして失敗させる）
const MAX_RENAME_ATTEMPTS: u32 = 1000;

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

/// 外部のファイルをプロジェクト配下へコピーする。
pub(crate) fn add_files(
    runner: &GitRunner,
    repo: &Path,
    sources: &[PathBuf],
    dest_subdir: &str,
    now: OffsetDateTime,
) -> Result<AddFilesOutcome, OpsError> {
    let (dest_dir, dest_rel) = resolve_dest_dir(repo, dest_subdir)?;

    // 計画: 何も変更せず、コピーできるものと断るものに分ける
    let mut outcome = AddFilesOutcome::default();
    let mut planned: Vec<(&PathBuf, String, u64)> = Vec::new();
    for src in sources {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let reject = |reason| RejectedFile {
            name: name.clone(),
            reason,
        };
        match std::fs::metadata(src) {
            Ok(m) if m.is_file() && !name.is_empty() => {
                if m.len() > LARGE_FILE_LIMIT_BYTES {
                    outcome
                        .rejected
                        .push(reject(AddRejectReason::TooLarge { size: m.len() }));
                } else {
                    planned.push((src, name.clone(), m.len()));
                }
            }
            Ok(_) => outcome.rejected.push(reject(AddRejectReason::NotAFile)),
            Err(_) => outcome.rejected.push(reject(AddRejectReason::Unreadable)),
        }
    }
    if planned.is_empty() {
        return Ok(outcome);
    }

    // 作業フォルダを変更する前に復元点を作る
    let branch = current_branch(runner, repo)?;
    let _ = create_restore_point(runner, repo, &branch, "add-files", now)?;

    for (src, name, size) in planned {
        match copy_without_overwrite(src, &dest_dir, &name) {
            Ok((final_name, renamed)) => {
                let path = if dest_rel.is_empty() {
                    final_name
                } else {
                    format!("{dest_rel}/{final_name}")
                };
                outcome.added.push(AddedFile {
                    path,
                    renamed,
                    large: size >= LARGE_FILE_WARN_BYTES,
                });
            }
            Err(_) => outcome.rejected.push(RejectedFile {
                name,
                reason: AddRejectReason::Unreadable,
            }),
        }
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
}
