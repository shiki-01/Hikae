// プロジェクトとして登録するフォルダが「広すぎない」かの判定（設計書 13.1、5.1 `project_folder_too_broad`）。
//
// ホームフォルダ、ドライブのルート、OS の標準フォルダ（デスクトップ・ドキュメント・ダウンロードなど）、
// システムフォルダを、そのまま登録すると、無関係な大量のファイルを監視・保存の対象にしてしまう。
// 基準になるパスは引数で受け取り（アプリが OS から得る）、ファイルシステムは見ない純関数にする。
//
// - ホーム・標準フォルダは、そのもの**だけ**を拒否する（配下のサブフォルダは可）
// - ドライブのルート（`C:\`、`/`、`/Volumes/<名前>`）も、そのものだけを拒否する
// - システムフォルダは、そのものと配下をすべて拒否する（OS が管理する場所をプロジェクトにしないため）
// - 比較は大文字小文字を区別せず、`.` と `..`、末尾の区切りを無視し、Windows の拡張パス
//   （`\\?\C:\`）は通常のドライブ表記と同じに扱う

use std::path::{Component, Path, PathBuf, Prefix};

/// 拒否する理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BroadFolderReason {
    /// ユーザーのホームフォルダそのもの（またはその親の `C:\Users` など）
    Home,
    /// ドライブのルート
    DriveRoot,
    /// デスクトップ・ドキュメントなど、OS の標準フォルダそのもの
    StandardFolder,
    /// システムフォルダ（配下を含む）
    SystemFolder,
}

/// 判定の基準になるパス。アプリが OS から得て渡す。
#[derive(Debug, Clone, Default)]
pub struct BroadFolders {
    /// ユーザーのホームフォルダ
    pub home: Option<PathBuf>,
    /// デスクトップ・ドキュメント・ダウンロード・ピクチャなどの標準フォルダ
    pub standard: Vec<PathBuf>,
    /// システムフォルダ（そのものと配下を拒否する）
    pub system: Vec<PathBuf>,
}

/// 比較用の正規形（成分を小文字にしたもの）。`.` は除き、`..` は 1 つ戻る。
pub(crate) fn key_of(path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                let text = match prefix.kind() {
                    Prefix::Disk(d) | Prefix::VerbatimDisk(d) => {
                        format!("{}:", (d as char).to_ascii_lowercase())
                    }
                    _ => prefix.as_os_str().to_string_lossy().to_lowercase(),
                };
                parts.push(text);
            }
            Component::RootDir => parts.push("/".to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            Component::Normal(name) => parts.push(name.to_string_lossy().to_lowercase()),
        }
    }
    parts
}

/// ドライブのルートか（`/`、`C:\`、macOS の `/Volumes/<名前>`）
fn is_drive_root(key: &[String]) -> bool {
    match key {
        [] => true,
        [only] => only == "/" || only.ends_with(':'),
        [prefix, root] => prefix.ends_with(':') && root == "/",
        [root, volumes, _name] => root == "/" && volumes == "volumes",
        _ => false,
    }
}

/// 登録しようとするフォルダが広すぎないか確かめる。広すぎる場合はその理由を返す。
pub fn check_project_folder(path: &Path, bases: &BroadFolders) -> Result<(), BroadFolderReason> {
    let key = key_of(path);
    if is_drive_root(&key) {
        return Err(BroadFolderReason::DriveRoot);
    }
    if let Some(home) = &bases.home {
        let home_key = key_of(home);
        if key == home_key {
            return Err(BroadFolderReason::Home);
        }
        // ホームの親（`C:\Users`、`/Users`）も、他の利用者のフォルダを巻き込むため拒否する
        if home_key.len() > 1 && key == home_key[..home_key.len() - 1] {
            return Err(BroadFolderReason::Home);
        }
    }
    if bases.standard.iter().any(|s| key_of(s) == key) {
        return Err(BroadFolderReason::StandardFolder);
    }
    if bases.system.iter().any(|s| {
        let system_key = key_of(s);
        !system_key.is_empty() && key.starts_with(&system_key)
    }) {
        return Err(BroadFolderReason::SystemFolder);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases() -> BroadFolders {
        let (home, system): (PathBuf, Vec<PathBuf>) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\Users\taro"),
                vec![
                    PathBuf::from(r"C:\Windows"),
                    PathBuf::from(r"C:\Program Files"),
                ],
            )
        } else {
            (
                PathBuf::from("/Users/taro"),
                vec![PathBuf::from("/System"), PathBuf::from("/usr")],
            )
        };
        BroadFolders {
            standard: vec![
                home.join("Desktop"),
                home.join("Documents"),
                home.join("Downloads"),
            ],
            home: Some(home),
            system,
        }
    }

    fn top_level(rest: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:\{}", rest.replace('/', "\\")))
        } else {
            PathBuf::from(format!("/{rest}"))
        }
    }

    #[test]
    fn home_and_its_parent_are_refused_but_subfolders_are_allowed() {
        let b = bases();
        let home = b.home.clone().expect("home");
        assert_eq!(
            check_project_folder(&home, &b),
            Err(BroadFolderReason::Home)
        );
        assert_eq!(
            check_project_folder(home.parent().expect("parent"), &b),
            Err(BroadFolderReason::Home)
        );
        assert_eq!(check_project_folder(&home.join("卒業論文"), &b), Ok(()));
        assert_eq!(
            check_project_folder(&home.join("Documents").join("卒業論文"), &b),
            Ok(())
        );
    }

    #[test]
    fn standard_folders_themselves_are_refused() {
        let b = bases();
        let home = b.home.clone().expect("home");
        for name in ["Desktop", "Documents", "Downloads"] {
            assert_eq!(
                check_project_folder(&home.join(name), &b),
                Err(BroadFolderReason::StandardFolder),
                "{name}"
            );
        }
        // 大文字小文字・`.` / `..` を含んでも同じ
        let sneaky = home.join("documents").join(".").join("x").join("..");
        assert_eq!(
            check_project_folder(&sneaky, &b),
            Err(BroadFolderReason::StandardFolder)
        );
    }

    #[test]
    fn drive_roots_are_refused() {
        let b = bases();
        let top = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        assert_eq!(
            check_project_folder(&top, &b),
            Err(BroadFolderReason::DriveRoot)
        );
        assert_eq!(
            check_project_folder(Path::new(""), &b),
            Err(BroadFolderReason::DriveRoot)
        );
        // ドライブの直下のフォルダは可
        assert_eq!(check_project_folder(&top_level("work"), &b), Ok(()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_forms_are_compared_alike() {
        let b = bases();
        assert_eq!(
            check_project_folder(Path::new(r"\\?\C:\"), &b),
            Err(BroadFolderReason::DriveRoot)
        );
        assert_eq!(
            check_project_folder(Path::new(r"c:\users\TARO"), &b),
            Err(BroadFolderReason::Home)
        );
        assert_eq!(
            check_project_folder(Path::new(r"\\?\C:\Users\taro\Desktop"), &b),
            Err(BroadFolderReason::StandardFolder)
        );
        assert_eq!(
            check_project_folder(Path::new(r"D:\"), &b),
            Err(BroadFolderReason::DriveRoot)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn mac_volume_roots_are_refused() {
        let b = bases();
        assert_eq!(
            check_project_folder(Path::new("/Volumes/Backup"), &b),
            Err(BroadFolderReason::DriveRoot)
        );
        assert_eq!(
            check_project_folder(Path::new("/Volumes/Backup/work"), &b),
            Ok(())
        );
    }

    #[test]
    fn system_folders_and_everything_under_them_are_refused() {
        let b = bases();
        let sys = b.system[0].clone();
        assert_eq!(
            check_project_folder(&sys, &b),
            Err(BroadFolderReason::SystemFolder)
        );
        assert_eq!(
            check_project_folder(&sys.join("Temp").join("x"), &b),
            Err(BroadFolderReason::SystemFolder)
        );
        // 名前の先頭が同じだけの別のフォルダは可
        let sibling = sys.with_file_name(format!(
            "{}-extra",
            sys.file_name().expect("name").to_string_lossy()
        ));
        assert_eq!(check_project_folder(&sibling, &b), Ok(()));
    }

    #[test]
    fn nothing_is_refused_without_bases_except_roots() {
        let none = BroadFolders::default();
        assert_eq!(check_project_folder(&top_level("work/a"), &none), Ok(()));
    }
}
