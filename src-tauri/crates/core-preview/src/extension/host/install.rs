// 利用者の追加フォルダの読み取り・取り込み・一覧からの除外（設計書 11.9）。
//
// - `read_summary`: マニフェストを読むだけ。無効の拡張も一覧に出すため、wasm や表示の入口の実体は見ない
// - `install_from_folder`: 選ばれたフォルダを `<利用者の拡張フォルダ>/<id>/` へ再帰コピーする。
//   元のフォルダは読むだけで変更しない。シンボリックリンクは追わず無視する。大きさとファイル数に上限を置く
// - コピーが途中で失敗したら、できかけのフォルダは削除せず `_failed/` へ rename で退ける
// - `move_to_removed`: 「一覧から外す」。削除はせず `_removed/` へ rename する
// - `install_from_zip`: zip を `_staging/` に安全に展開してから `install_from_folder` に渡す（設計書 11.9）。
//   展開の前に、すべてのエントリを検査する（パス・シンボリックリンク・件数・大きさ・圧縮率）
// - ここには削除（remove_*）を置かない。登録（起動時）は app の load_extensions が行う
// - エラーには、パス・ファイルの内容を含めない（理由の種類だけ）

use super::manifest::{self, valid_id, ManifestError, MANIFEST_MAX_BYTES};
use super::{check_entry, read_entry, read_limited, MANIFEST_FILE};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive};

/// 取り込むフォルダ全体の大きさの上限（バイト）
pub const INSTALL_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 取り込むフォルダ全体のファイル数の上限
pub const INSTALL_MAX_FILES: usize = 2000;
/// zip のエントリ数の上限（ディレクトリも数える）
pub const ZIP_MAX_ENTRIES: usize = 2000;
/// zip の 1 ファイルの大きさの上限（展開後のバイト）
pub const ZIP_MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// 圧縮率の上限（展開後の大きさ / 圧縮後の大きさ）。超えるものは圧縮爆弾として拒否する
const ZIP_MAX_RATIO: u64 = 100;
/// 圧縮率を見る最小の大きさ（これ以下の小さいファイルは圧縮率を見ない）
const ZIP_RATIO_MIN_BYTES: u64 = 1024 * 1024;
/// zip の展開先の置き場（利用者の拡張フォルダの直下。展開が終わったら `_removed` か `_failed` へ移す）
const STAGING_DIR: &str = "_staging";
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
    /// zip として開けない、壊れている、暗号化されている、対応しない圧縮方式を使っている
    ArchiveUnreadable,
    /// zip の中身が安全でない（パスの規則違反、シンボリックリンク、同じ名前の重複、圧縮率の超過）
    ArchiveUnsafe,
    /// zip の展開に失敗した（展開途中のフォルダは `_failed` へ退けた）
    ExtractFailed,
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

/// zip を `user_ext_dir/_staging/<now>-<連番>/` に安全に展開し、`install_from_folder` で取り込む（設計書 11.9）。
///
/// - 展開の前に、すべてのエントリを検査する（どれか 1 つでも違反なら zip 全体を拒否し、何も作らない）
///   - 名前: 絶対パス・`..`・ドライブ文字（`:`）・先頭の `/` や `\`・NUL・制御文字・Windows の予約名・末尾のドットと空白は拒否する
///   - シンボリックリンク（Unix の外部属性で判定）、暗号化、対応しない圧縮方式は拒否する
///   - エントリ数 2000 以下、展開後の合計 64 MiB 以下、1 ファイル 32 MiB 以下、圧縮率 100 倍以下（1 MiB を超えるファイル）
///   - 同じ名前（大文字と小文字を区別しない）のファイルの重複は拒否する
/// - 取り込みの元のフォルダは、zip の直下に `extension.json` があればそのフォルダ、無ければ直下の唯一のフォルダの中にある場合のみ
/// - 展開は新規作成のみ（既存のファイルを上書きしない）
/// - 展開が失敗した、または取り込みが失敗したら、展開先を削除せず `_failed/` へ rename で退ける
/// - 取り込みが成功したら、展開先を `_removed/` へ rename で退ける（残さない）
pub fn install_from_zip(
    zip_path: &Path,
    user_ext_dir: &Path,
    builtin_ids: &[String],
    now: &str,
) -> Result<InstalledExtension, InstallError> {
    let file = File::open(zip_path).map_err(|_| InstallError::SourceUnreadable)?;
    if !file
        .metadata()
        .map_err(|_| InstallError::SourceUnreadable)?
        .is_file()
    {
        return Err(InstallError::SourceUnreadable);
    }
    let mut archive =
        ZipArchive::new(BufReader::new(file)).map_err(|_| InstallError::ArchiveUnreadable)?;
    // 展開の前に、すべてのエントリを検査する（何も作らない）
    let plan = plan_zip(&mut archive)?;

    fs::create_dir_all(user_ext_dir).map_err(|_| InstallError::ExtractFailed)?;
    let staging = create_staging(user_ext_dir, now)?;
    let result = extract_plan(&mut archive, &plan, &staging).and_then(|()| {
        let src = match &plan.root {
            Some(folder) => staging.join(folder),
            None => staging.clone(),
        };
        install_from_folder(&src, user_ext_dir, builtin_ids, now)
    });
    match result {
        Ok(installed) => {
            // 展開先は残さない。移せなくても取り込みは済んでいるため、結果は成功のまま返す
            let _ = park(user_ext_dir, REMOVED_DIR, "staging", &staging, now);
            Ok(installed)
        }
        Err(err) => {
            // 展開先は削除せず退ける。退避できなくても、元の失敗の理由を返す
            let _ = park(user_ext_dir, FAILED_DIR, "staging", &staging, now);
            Err(err)
        }
    }
}

/// 展開の計画。検査を通ったエントリだけを持つ（展開の前に作る）
struct ZipPlan {
    /// 作るディレクトリ（展開先からの相対パス）
    dirs: Vec<PathBuf>,
    /// 展開するファイル（zip のインデックス、展開先からの相対パス）
    files: Vec<(usize, PathBuf)>,
    /// 取り込みの元にするフォルダ名（zip の直下の唯一のフォルダに `extension.json` がある場合だけ Some）
    root: Option<String>,
}

/// zip の全エントリを検査し、展開の計画を作る（展開はしない）
fn plan_zip<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<ZipPlan, InstallError> {
    if archive.len() > ZIP_MAX_ENTRIES {
        return Err(InstallError::TooLarge);
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut top_level: BTreeSet<String> = BTreeSet::new();
    let mut root_manifest = false;
    let mut inner_manifest: Option<String> = None;
    let mut total: u64 = 0;

    for index in 0..archive.len() {
        // by_index_raw は中身を展開しない（検査だけ）
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| InstallError::ArchiveUnreadable)?;
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(InstallError::ArchiveUnreadable);
        }
        if entry.is_symlink() {
            return Err(InstallError::ArchiveUnsafe);
        }
        let is_dir = entry.is_dir();
        let parts = zip_components(entry.name(), is_dir).ok_or(InstallError::ArchiveUnsafe)?;
        if let Some(first) = parts.first() {
            top_level.insert(first.clone());
        }
        if !is_dir {
            if parts.len() == 1 && parts[0] == MANIFEST_FILE {
                root_manifest = true;
            }
            if parts.len() == 2 && parts[1] == MANIFEST_FILE {
                inner_manifest = Some(parts[0].clone());
            }
            let size = entry.size();
            if size > ZIP_MAX_FILE_BYTES {
                return Err(InstallError::TooLarge);
            }
            total = total.checked_add(size).ok_or(InstallError::TooLarge)?;
            if total > INSTALL_MAX_BYTES {
                return Err(InstallError::TooLarge);
            }
            if size > ZIP_RATIO_MIN_BYTES && !ratio_ok(size, entry.compressed_size()) {
                return Err(InstallError::ArchiveUnsafe);
            }
            // 大文字と小文字を区別しない（Windows と macOS の既定の場所で、同じファイルになるため）
            if !seen_files.insert(parts.join("/").to_lowercase()) {
                return Err(InstallError::ArchiveUnsafe);
            }
            files.push((index, parts.iter().collect()));
        } else {
            dirs.push(parts.iter().collect());
        }
    }

    // 取り込みの元: 直下に extension.json があるか、直下の唯一のフォルダの中にあるか
    let root = if root_manifest {
        None
    } else {
        match (top_level.len(), inner_manifest) {
            (1, Some(inner)) if top_level.contains(&inner) => Some(inner),
            _ => None,
        }
    };
    Ok(ZipPlan { dirs, files, root })
}

/// zip のエントリ名を、安全な要素の並びにする。違反があれば None。
/// 末尾の `/`（ディレクトリ）は 1 つだけ許す。
fn zip_components(name: &str, is_dir: bool) -> Option<Vec<String>> {
    let forbidden = |c: char| {
        c < ' ' || c == '\u{7f}' || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    };
    if name.is_empty() || name.chars().any(forbidden) {
        return None;
    }
    let trimmed = if is_dir {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    if trimmed.is_empty() {
        return None;
    }
    // 先頭の `/` は空の要素になるため、ここで拒否される
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if !safe_component(part) {
            return None;
        }
        parts.push(part.to_string());
    }
    Some(parts)
}

/// 要素 1 つが Windows でも安全な名前か（`.`・`..`・末尾のドットと空白・予約名を拒否する）
fn safe_component(part: &str) -> bool {
    if part.is_empty() || part == "." || part == ".." {
        return false;
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return false;
    }
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved_device = |prefix: &str| {
        stem.len() == 4 && stem.starts_with(prefix) && stem.as_bytes()[3].is_ascii_digit()
    };
    !(matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_device("COM")
        || reserved_device("LPT"))
}

/// 圧縮率が上限以下か（圧縮後が 0 のものは、大きさがあれば超過とみなす）
fn ratio_ok(size: u64, compressed: u64) -> bool {
    compressed > 0 && u128::from(size) <= u128::from(compressed) * u128::from(ZIP_MAX_RATIO)
}

/// 展開先を `_staging/<now>-<連番>/` に新規作成する（既存のフォルダは使わない）
fn create_staging(user_ext_dir: &Path, now: &str) -> Result<PathBuf, InstallError> {
    let parent = user_ext_dir.join(STAGING_DIR);
    fs::create_dir_all(&parent).map_err(|_| InstallError::ExtractFailed)?;
    let stamp = stamp_of(now);
    for n in 1..=TARGET_NAME_TRIES {
        let candidate = parent.join(format!("{stamp}-{n}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(InstallError::ExtractFailed),
        }
    }
    Err(InstallError::ExtractFailed)
}

/// 計画どおりに展開する。ファイルは新規作成のみ（上書きしない）。実際に読んだ量でも上限を確かめる
fn extract_plan<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    plan: &ZipPlan,
    staging: &Path,
) -> Result<(), InstallError> {
    for dir in &plan.dirs {
        fs::create_dir_all(staging.join(dir)).map_err(|_| InstallError::ExtractFailed)?;
    }
    let mut total: u64 = 0;
    for (index, rel) in &plan.files {
        let path = staging.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| InstallError::ExtractFailed)?;
        }
        let mut reader = archive
            .by_index(*index)
            .map_err(|_| InstallError::ExtractFailed)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| InstallError::ExtractFailed)?;
        // 宣言された大きさは偽れるため、上限 + 1 バイトまでしか読まない
        let written = io::copy(&mut (&mut reader).take(ZIP_MAX_FILE_BYTES + 1), &mut out)
            .map_err(|_| InstallError::ExtractFailed)?;
        if written > ZIP_MAX_FILE_BYTES {
            return Err(InstallError::TooLarge);
        }
        total = total.saturating_add(written);
        if total > INSTALL_MAX_BYTES {
            return Err(InstallError::TooLarge);
        }
    }
    Ok(())
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
