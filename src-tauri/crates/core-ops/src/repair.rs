// 破損リポジトリの復旧（設計書 5.2 E10「取得し直す」）のうち、Tauri と git に依存しないファイル操作。
//
// 手順のうち、ここで扱うのは次の部分だけ（git clone と画面・ジャーナルは呼び出し側が行う）。
// - 手順 1: 作業フォルダを `<名前>_退避_<日時>` へ rename で移動する（コピーも削除もしない）
// - 手順 3: 退避フォルダの未保存の内容を、新しい作業フォルダへコピーし直す
// - 手順 4: 失敗したとき、できかけのフォルダを `<名前>_取得失敗_<日時>` へ退け、退避フォルダを元の名前へ戻す
//
// このモジュールのどの関数も、削除（remove_file / remove_dir 系）を呼ばない。退避フォルダの中身は変更しない。

use crate::models::OpsError;
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirEntry, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

/// 退避フォルダ名の区切り（`<名前>_退避_<日時>`）
const ASIDE_MARK: &str = "_退避_";

/// 取得失敗のフォルダ名の区切り（`<名前>_取得失敗_<日時>`）
const FAILED_MARK: &str = "_取得失敗_";

/// 内容の比較で一度に読むバイト数
const COMPARE_CHUNK: usize = 64 * 1024;

/// 作業フォルダを同じ親フォルダの `<フォルダ名>_退避_<now_local>` へ移動する（設計書 5.2 E10 手順 1）。
///
/// `now_local` は `YYYYMMDD-HHMMSS` の形式（15 文字）。同名があれば `-1`、`-2`… を付ける。
/// `rename` だけを使い、コピーも削除もしない。移動先は、移動の直前に空きを確かめる。
pub fn move_aside(project_dir: &Path, now_local: &str) -> Result<PathBuf, OpsError> {
    if !is_timestamp(now_local) {
        return Err(OpsError::InvalidInput(format!(
            "timestamp must be YYYYMMDD-HHMMSS: {now_local}"
        )));
    }
    let name = project_dir
        .file_name()
        .ok_or_else(|| OpsError::InvalidInput("project folder has no name".to_string()))?;
    let parent = project_dir
        .parent()
        .ok_or_else(|| OpsError::InvalidInput("project folder has no parent".to_string()))?;

    let target = free_sibling(parent, name, &format!("{ASIDE_MARK}{now_local}"))?;
    fs::rename(project_dir, &target)?;
    Ok(target)
}

/// 退避フォルダを元の名前へ戻す（設計書 5.2 E10 手順 4）。
///
/// `original` が存在する場合（できかけの `git clone` の結果）は、先に `<名前>_取得失敗_<日時>` へ退けてから
/// `aside` を `original` へ戻す。`<日時>` は退避フォルダ名の `_退避_` より後ろの部分（`-N` を含む）を使う。
/// 削除はしない。途中で失敗したときは、そこまでの rename の結果がそのまま残る（呼び出し側が場所を案内する）。
pub fn restore_aside(aside: &Path, original: &Path) -> Result<(), OpsError> {
    let aside_name = aside
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| OpsError::InvalidInput("aside folder name is not valid text".to_string()))?;
    let stamp = aside_name
        .rfind(ASIDE_MARK)
        .map(|i| &aside_name[i + ASIDE_MARK.len()..])
        .filter(|s| !s.is_empty())
        .ok_or_else(|| OpsError::InvalidInput(format!("not an aside folder name: {aside_name}")))?;

    let original_name = original
        .file_name()
        .ok_or_else(|| OpsError::InvalidInput("original path has no name".to_string()))?;
    let parent = original
        .parent()
        .ok_or_else(|| OpsError::InvalidInput("original path has no parent".to_string()))?;

    // できかけの取得結果があれば退けて、退避フォルダの置き場を空ける
    match fs::symlink_metadata(original) {
        Ok(_) => {
            let parked = free_sibling(parent, original_name, &format!("{FAILED_MARK}{stamp}"))?;
            fs::rename(original, parked)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    fs::rename(aside, original)?;
    Ok(())
}

/// 未保存の内容を退避フォルダから戻した結果（設計書 5.2 E10 手順 3）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyBackReport {
    /// 新しい作業フォルダへコピーしたファイルの数
    pub copied: u32,
    /// 新しい作業フォルダに同じ内容のファイルがあったため、コピーしなかった数
    pub skipped_same: u32,
    /// コピーできなかったファイルの相対パス（`/` 区切り）。退避フォルダには残っている
    pub failed: Vec<String>,
}

/// 退避フォルダの `.git` を除くすべての通常ファイルのうち、`new_dir` に無いもの、
/// または内容（サイズ、次にバイト列）が違うものを `new_dir` へコピーする（設計書 5.2 E10 手順 3）。
///
/// - ルート直下の `.git`（ファイルでもフォルダでも）だけを除く。ネストした `.git` はコピーする
/// - シンボリックリンクは追わず、無視する
/// - コピーに失敗したファイルは `failed` に入れて続行する。読めない子フォルダは、そのフォルダの相対パスを `failed` に入れる
/// - `new_dir` 側のファイルは削除しない。`new_dir` 側がリンクやフォルダの場合は書き換えず、`failed` に入れる
/// - 退避フォルダ（`aside`）の内容は変更しない
pub fn copy_back_unsaved(aside: &Path, new_dir: &Path) -> Result<CopyBackReport, OpsError> {
    let mut report = CopyBackReport::default();
    // 退避フォルダ自体が読めないときは、何もコピーせずにエラーを返す
    let entries = read_entries(aside)?;
    walk(entries, new_dir, "", true, &mut report);
    Ok(report)
}

/// 1 階層ぶんをコピーする。子フォルダは再帰で処理し、読めなければ親が `failed` に入れる。
fn walk(
    entries: Vec<DirEntry>,
    dst_dir: &Path,
    rel_prefix: &str,
    is_root: bool,
    report: &mut CopyBackReport,
) {
    for entry in entries {
        let name = entry.file_name();
        if is_root && name == OsStr::new(".git") {
            continue;
        }
        let name_text = name.to_string_lossy();
        let rel = if rel_prefix.is_empty() {
            name_text.into_owned()
        } else {
            format!("{rel_prefix}/{name_text}")
        };

        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => {
                report.failed.push(rel);
                continue;
            }
        };
        // シンボリックリンクは追わない（無視する）
        if file_type.is_symlink() {
            continue;
        }

        let src = entry.path();
        let dst = dst_dir.join(&name);
        if file_type.is_dir() {
            match read_entries(&src) {
                Ok(children) => walk(children, &dst, &rel, false, report),
                Err(_) => report.failed.push(rel),
            }
        } else if file_type.is_file() {
            match copy_if_needed(&src, &dst, dst_dir) {
                Ok(true) => report.copied += 1,
                Ok(false) => report.skipped_same += 1,
                Err(_) => report.failed.push(rel),
            }
        }
        // 通常のファイル・フォルダ以外（ソケットなど）は対象外として無視する
    }
}

/// フォルダの直下の項目を、名前の順で読む
fn read_entries(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries: Vec<DirEntry> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

/// `src` を `dst` へコピーする。同じ内容が既にあれば何もせず `false`、コピーしたら `true`。
/// `dst` がリンクやフォルダなら書き換えず、エラーにする。
fn copy_if_needed(src: &Path, dst: &Path, dst_dir: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(dst) {
        Ok(meta) => {
            if !meta.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination is not a regular file",
                ));
            }
            if same_content(src, dst)? {
                return Ok(false);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::create_dir_all(dst_dir)?;
    fs::copy(src, dst)?;
    Ok(true)
}

/// 2 つのファイルの内容が同じか。サイズを先に比べ、同じときだけバイト列を比べる。
fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut fa = BufReader::new(File::open(a)?);
    let mut fb = BufReader::new(File::open(b)?);
    let mut buf_a = vec![0u8; COMPARE_CHUNK];
    let mut buf_b = vec![0u8; COMPARE_CHUNK];
    loop {
        let na = fill(&mut fa, &mut buf_a)?;
        let nb = fill(&mut fb, &mut buf_b)?;
        if na != nb || buf_a[..na] != buf_b[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// バッファが埋まるか、ファイルの終わりまで読む（途中で短く返されても続けて読む）
fn fill(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// `parent` の中で `<base><suffix>` の空いている名前を返す。埋まっていれば `-1`、`-2`… を付ける。
/// 確認するだけで、作成はしない（直後の rename で使う）。
fn free_sibling(parent: &Path, base: &OsStr, suffix: &str) -> io::Result<PathBuf> {
    let mut n: u32 = 0;
    loop {
        let mut name = OsString::from(base);
        name.push(suffix);
        if n > 0 {
            name.push(format!("-{n}"));
        }
        let candidate = parent.join(name);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => n += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(e) => return Err(e),
        }
    }
}

/// `YYYYMMDD-HHMMSS`（15 文字、8 文字目がハイフン）か
fn is_timestamp(s: &str) -> bool {
    s.len() == 15
        && s.bytes().enumerate().all(|(i, b)| {
            if i == 8 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}
