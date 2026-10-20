// 利用者の追加フォルダの読み取り・取り込み・一覧からの除外（設計書 11.9）。
//
// - `read_summary`: マニフェストを読むだけ。無効の拡張も一覧に出すため、wasm や表示の入口の実体は見ない
// - `install_from_folder`: 選ばれたフォルダを `<利用者の拡張フォルダ>/<id>/` へ再帰コピーする。
//   元のフォルダは読むだけで変更しない。シンボリックリンクは追わず無視する。大きさとファイル数に上限を置く
// - コピーが途中で失敗したら、できかけのフォルダは削除せず `_failed/` へ rename で退ける
// - `move_to_removed`: 「一覧から外す」。削除はせず `_removed/` へ rename する
// - ここには削除（remove_*）を置かない。登録（起動時）は app の load_extensions が行う
// - エラーには、パス・ファイルの内容を含めない（理由の種類だけ）

use super::manifest::{self, valid_id, ManifestError, MANIFEST_MAX_BYTES};
use super::{check_entry, read_entry, read_limited, MANIFEST_FILE};
use std::fs;
use std::path::{Path, PathBuf};

/// 取り込むフォルダ全体の大きさの上限（バイト）
pub const INSTALL_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 取り込むフォルダ全体のファイル数の上限
pub const INSTALL_MAX_FILES: usize = 2000;
/// コピーに失敗したフォルダの退避先（利用者の拡張フォルダの直下）
const FAILED_DIR: &str = "_failed";
/// 一覧から外した拡張の退避先（利用者の拡張フォルダの直下）
const REMOVED_DIR: &str = "_removed";
/// 退避先の名前を探す試行の上限
const TARGET_NAME_TRIES: usize = 100;
/// 退避先の名前に使う日時の長さの上限（文字数）
const STAMP_MAX_CHARS: usize = 64;

/// マニフェストの概要（一覧の表示用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    /// 対応する拡張子（表示と抽出器の両方。`.` を除いた英小文字。全部は `*`）。重複は無い
    pub file_types: Vec<String>,
}

/// 取り込んだ拡張
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// 取り込み・除外の失敗の理由。どの種類も、パス・ファイルの内容を含まない
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallError {
    /// 選ばれたフォルダを読めない（無い、フォルダではない、中を一覧できない）
    SourceUnreadable,
    /// マニフェストまたは entry の検証に失敗した（理由は `ManifestError`）
    Manifest(ManifestError),
    /// 標準の拡張と同じ ID（標準の ID は拒否する）
    BuiltinId,
    /// 同じ ID の追加分がすでにある
    DuplicateId,
    /// フォルダが大きすぎる（総サイズ 64 MiB、ファイル数 2000 を超える）
    TooLarge,
    /// コピーに失敗した（できかけのフォルダは `_failed` へ退けようとした）
    CopyFailed,
    /// 退避フォルダへの移動に失敗した（移動できなかったため、フォルダはそのまま）
    MoveFailed,
    /// 一覧から外す対象のフォルダが無い
    NotFound,
    /// ID の形式が不正
    InvalidId,
}

/// マニフェストを読み、概要を返す。宣言された entry の実体は確かめない（一覧のための読み取り）
pub fn read_summary(dir: &Path) -> Result<ExtensionSummary, ManifestError> {
    let bytes = read_limited(&dir.join(MANIFEST_FILE), MANIFEST_MAX_BYTES)?;
    let parsed = manifest::parse(&bytes)?;
    let mut file_types: Vec<String> = Vec::new();
    let renderer_types = parsed.renderers.iter().flat_map(|r| r.file_types.iter());
    let extractor_types = parsed.extractors.iter().flat_map(|x| x.file_types.iter());
    for ty in renderer_types.chain(extractor_types) {
        if !file_types.contains(ty) {
            file_types.push(ty.clone());
        }
    }
    Ok(ExtensionSummary {
        id: parsed.id,
        name: parsed.name,
        version: parsed.version.to_string(),
        file_types,
    })
}

/// 選ばれたフォルダを `user_ext_dir/<id>/` へ取り込む。元のフォルダは変更しない。
///
/// - マニフェストと宣言された entry を、登録時と同じ検査で確かめる
/// - `builtin_ids` にある ID（標準の拡張）と、`user_ext_dir` に既にある ID は拒否する
/// - コピーの後にも entry を確かめる（シンボリックリンクは飛ばすため、取り込み先で揃っているかを見る）
/// - `now` は退避先の名前に使う（記号は `-` に置き換える）
///
/// 登録の記録（有効の行）と、次回起動での登録は呼び出し側（app）が行う。
pub fn install_from_folder(
    src: &Path,
    user_ext_dir: &Path,
    builtin_ids: &[String],
    now: &str,
) -> Result<InstalledExtension, InstallError> {
    let src_root = src
        .canonicalize()
        .map_err(|_| InstallError::SourceUnreadable)?;
    if !src_root.is_dir() {
        return Err(InstallError::SourceUnreadable);
    }
    let bytes = read_limited(&src_root.join(MANIFEST_FILE), MANIFEST_MAX_BYTES)
        .map_err(InstallError::Manifest)?;
    let parsed = manifest::parse(&bytes).map_err(InstallError::Manifest)?;

    if builtin_ids.iter().any(|id| id == &parsed.id) {
        return Err(InstallError::BuiltinId);
    }
    let dest = user_ext_dir.join(&parsed.id);
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(InstallError::DuplicateId);
    }

    // 宣言された entry の実体を確かめる（登録時と同じ検査）
    for renderer in &parsed.renderers {
        check_entry(&src_root, &renderer.entry).map_err(InstallError::Manifest)?;
    }
    for extractor in &parsed.extractors {
        read_entry(&src_root, &extractor.entry).map_err(InstallError::Manifest)?;
    }

    // コピーの前に、大きさとファイル数を確かめる（超えるなら何も作らない）
    let plan = scan(&src_root)?;

    fs::create_dir_all(user_ext_dir).map_err(|_| InstallError::CopyFailed)?;
    match fs::create_dir(&dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(InstallError::DuplicateId)
        }
        Err(_) => return Err(InstallError::CopyFailed),
    }

    if copy_plan(&plan, &dest).is_err() {
        // できかけのフォルダは消さず、退避する。退避できなくても、元の失敗の理由を返す
        let _ = park(user_ext_dir, FAILED_DIR, &parsed.id, &dest, now);
        return Err(InstallError::CopyFailed);
    }
    if let Err(err) = verify_entries(&dest, &parsed) {
        let _ = park(user_ext_dir, FAILED_DIR, &parsed.id, &dest, now);
        return Err(InstallError::Manifest(err));
    }

    Ok(InstalledExtension {
        id: parsed.id,
        name: parsed.name,
        version: parsed.version.to_string(),
    })
}

/// 「一覧から外す」。`user_ext_dir/<id>/` を `_removed/<id>_<now>/` へ移動する（削除はしない）。
/// 標準の拡張は `user_ext_dir` に無いため、呼び出し側が先に拒否する。
pub fn move_to_removed(user_ext_dir: &Path, id: &str, now: &str) -> Result<(), InstallError> {
    if !valid_id(id) {
        return Err(InstallError::InvalidId);
    }
    let from = user_ext_dir.join(id);
    // リンクは対象外（フォルダの実体だけを移す）
    let meta = fs::symlink_metadata(&from).map_err(|_| InstallError::NotFound)?;
    if !meta.is_dir() {
        return Err(InstallError::NotFound);
    }
    let parent = user_ext_dir.join(REMOVED_DIR);
    fs::create_dir_all(&parent).map_err(|_| InstallError::MoveFailed)?;
    let target = unused_target(&parent, id, now)?;
    fs::rename(&from, &target).map_err(|_| InstallError::MoveFailed)?;
    Ok(())
}

/// コピーする内容の計画。元のフォルダを 1 回だけ走査して作る
struct Plan {
    /// 作る順に並べた、フォルダからの相対パス（親が先）
    dirs: Vec<PathBuf>,
    /// コピーするファイル（元の絶対パス、フォルダからの相対パス）
    files: Vec<(PathBuf, PathBuf)>,
}

/// 元のフォルダを走査し、コピーの計画を作る。上限を超えたら何も作らずに `TooLarge`
fn scan(root: &Path) -> Result<Plan, InstallError> {
    let mut plan = Plan {
        dirs: Vec::new(),
        files: Vec::new(),
    };
    let mut total: u64 = 0;
    scan_dir(root, Path::new(""), &mut plan, &mut total)?;
    Ok(plan)
}

fn scan_dir(root: &Path, rel: &Path, plan: &mut Plan, total: &mut u64) -> Result<(), InstallError> {
    let entries = fs::read_dir(root.join(rel)).map_err(|_| InstallError::SourceUnreadable)?;
    for entry in entries {
        let entry = entry.map_err(|_| InstallError::SourceUnreadable)?;
        let abs = entry.path();
        let rel_path = rel.join(entry.file_name());
        // リンクは追わず、そのまま無視する（リンク先のファイルを取り込まないため）
        let meta = fs::symlink_metadata(&abs).map_err(|_| InstallError::SourceUnreadable)?;
        let kind = meta.file_type();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            plan.dirs.push(rel_path.clone());
            scan_dir(root, &rel_path, plan, total)?;
        } else if kind.is_file() {
            *total = total.saturating_add(meta.len());
            if plan.files.len() >= INSTALL_MAX_FILES || *total > INSTALL_MAX_BYTES {
                return Err(InstallError::TooLarge);
            }
            plan.files.push((abs, rel_path));
        }
    }
    Ok(())
}

/// 計画どおりにフォルダとファイルを作る（フォルダは親から順に作る）
fn copy_plan(plan: &Plan, dest: &Path) -> std::io::Result<()> {
    for dir in &plan.dirs {
        fs::create_dir(dest.join(dir))?;
    }
    for (from, rel) in &plan.files {
        fs::copy(from, dest.join(rel))?;
    }
    Ok(())
}

/// 取り込み先で、宣言された entry が揃っているかを確かめる（登録時と同じ検査）
fn verify_entries(dest: &Path, parsed: &manifest::Manifest) -> Result<(), ManifestError> {
    let root = dest.canonicalize().map_err(|_| ManifestError::Unreadable)?;
    for renderer in &parsed.renderers {
        check_entry(&root, &renderer.entry)?;
    }
    for extractor in &parsed.extractors {
        read_entry(&root, &extractor.entry)?;
    }
    Ok(())
}

/// `from` を `<拡張フォルダ>/<folder>/<id>_<now>/` へ移す（rename のみ）
fn park(
    user_ext_dir: &Path,
    folder: &str,
    id: &str,
    from: &Path,
    now: &str,
) -> Result<PathBuf, InstallError> {
    let parent = user_ext_dir.join(folder);
    fs::create_dir_all(&parent).map_err(|_| InstallError::MoveFailed)?;
    let target = unused_target(&parent, id, now)?;
    fs::rename(from, &target).map_err(|_| InstallError::MoveFailed)?;
    Ok(target)
}

/// `<id>_<now>` の名前で、まだ無いパスを返す（同じ日時の退避が重なったら `_2`、`_3`… を付ける）
fn unused_target(parent: &Path, id: &str, now: &str) -> Result<PathBuf, InstallError> {
    let stamp = stamp_of(now);
    for n in 1..=TARGET_NAME_TRIES {
        let name = if n == 1 {
            format!("{id}_{stamp}")
        } else {
            format!("{id}_{stamp}_{n}")
        };
        let candidate = parent.join(name);
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(InstallError::MoveFailed)
}

/// 日時の文字列をフォルダ名に使える形にする（英数字以外は `-`。空なら `0`）
fn stamp_of(now: &str) -> String {
    let stamp: String = now
        .chars()
        .take(STAMP_MAX_CHARS)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if stamp.is_empty() {
        "0".to_string()
    } else {
        stamp
    }
}
