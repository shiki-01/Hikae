// クラウド同期フォルダ上のプロジェクトの検出（設計書 13.1「クラウド同期フォルダ」）。
//
// OneDrive・iCloud Drive・Dropbox・Google Drive などの同期ソフトの対象フォルダに、プロジェクトを置くと、
// 同期ソフトが `.git` の中のファイルを書き換えたり、同時に別の PC の版を混ぜたりして、リポジトリが
// 壊れることがある。そのため、登録時に警告する（拒否はしない。選ぶのは利用者）。
//
// 判定は文字列だけを見る純関数にする（ファイルシステムは見ない）。次の 2 つを併用する。
//
// 1. 基準のパス（`CloudRoots`）: アプリが環境変数（`OneDrive`、`OneDriveConsumer`、`OneDriveCommercial`）と
//    ホームフォルダ（`~/Dropbox`、`~/Google Drive`、`~/Library/Mobile Documents`、`~/iCloudDrive` など）から
//    作って渡す。その配下なら同期フォルダ
// 2. フォルダ名: パスのどこかに、同期ソフトが作る名前（`OneDrive`、`OneDrive - 会社名`、`Dropbox`、
//    `Google Drive`、`My Drive`、`iCloud Drive`、`Mobile Documents` など）が含まれる。基準のパスを
//    得られない場合や、環境変数に出ない形（macOS の `~/Library/CloudStorage/...`）を拾う
//
// 比較は大文字小文字を区別せず、`.` と `..`、末尾の区切りを無視し、Windows の拡張パス（`\\?\C:\`）は
// 通常のドライブ表記と同じに扱う（`broad_folder` と同じ正規化）。

use crate::broad_folder::key_of;
use std::path::{Path, PathBuf};

/// 同期サービスの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudSyncService {
    OneDrive,
    ICloud,
    Dropbox,
    GoogleDrive,
}

/// 判定の基準になるパス。アプリが環境変数とホームフォルダから作って渡す。
#[derive(Debug, Clone, Default)]
pub struct CloudRoots {
    /// 同期フォルダの場所（その配下を同期フォルダとみなす）
    pub roots: Vec<(CloudSyncService, PathBuf)>,
}

/// フォルダ名（小文字）から同期サービスを推定する。
fn service_of_name(name: &str) -> Option<CloudSyncService> {
    // OneDrive: `OneDrive`、`OneDrive - 会社名`、`OneDrive-Personal`（macOS の CloudStorage）
    if name == "onedrive" || name.starts_with("onedrive - ") || name.starts_with("onedrive-") {
        return Some(CloudSyncService::OneDrive);
    }
    // Dropbox: `Dropbox`、`Dropbox (Personal)`、`Dropbox-Personal`
    if name == "dropbox" || name.starts_with("dropbox (") || name.starts_with("dropbox-") {
        return Some(CloudSyncService::Dropbox);
    }
    // Google Drive: `Google Drive`、`GoogleDrive`、`GoogleDrive-<アカウント>`（macOS）、`My Drive`
    if matches!(name, "google drive" | "googledrive" | "my drive")
        || (name.starts_with("googledrive-") && name.contains('@'))
    {
        return Some(CloudSyncService::GoogleDrive);
    }
    // iCloud Drive: `iCloud Drive`、`iCloudDrive`（Windows）、`Mobile Documents`（macOS）
    if matches!(
        name,
        "icloud drive" | "iclouddrive" | "mobile documents" | "com~apple~clouddocs"
    ) {
        return Some(CloudSyncService::ICloud);
    }
    None
}

/// `path` が同期フォルダの配下なら、その同期サービスを返す。
///
/// 基準のパスでの一致を先に見て、無ければフォルダ名で推定する。
pub fn detect_cloud_sync(path: &Path, roots: &CloudRoots) -> Option<CloudSyncService> {
    let key = key_of(path);
    for (service, root) in &roots.roots {
        let root_key = key_of(root);
        // ドライブのルートや `/` のような名前を含まない基準は、すべてを同期フォルダにしてしまうため使わない
        let named_parts = root_key
            .iter()
            .filter(|part| part.as_str() != "/" && !part.ends_with(':'))
            .count();
        if named_parts >= 1 && key.starts_with(&root_key) {
            return Some(*service);
        }
    }
    key.iter().find_map(|part| service_of_name(part))
}

/// 実体（リンクの先）も含めて判定する。どちらかが同期フォルダなら、その種類を返す。
pub fn detect_cloud_sync_with_real(
    path: &Path,
    real: Option<&Path>,
    roots: &CloudRoots,
) -> Option<CloudSyncService> {
    detect_cloud_sync(path, roots).or_else(|| real.and_then(|r| detect_cloud_sync(r, roots)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use CloudSyncService::*;

    /// OS に依らずに組み立てるパス（Windows は \、それ以外は \ から始める）
    fn p(parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
        for part in parts {
            path.push(part);
        }
        path
    }

    fn roots() -> CloudRoots {
        CloudRoots {
            roots: vec![
                (OneDrive, p(&["Sync", "OD"])),
                (OneDrive, p(&["Users", "ito", "OneDrive - 研究室"])),
                (Dropbox, p(&["Users", "ito", "Dropbox"])),
                (ICloud, p(&["Users", "ito", "Library", "Mobile Documents"])),
                (GoogleDrive, p(&["Users", "ito", "Google Drive"])),
            ],
        }
    }

    fn detect(parts: &[&str]) -> Option<CloudSyncService> {
        detect_cloud_sync(&p(parts), &roots())
    }

    fn by_name(parts: &[&str]) -> Option<CloudSyncService> {
        detect_cloud_sync(&p(parts), &CloudRoots::default())
    }

    #[test]
    fn folders_under_a_base_path_are_sync_folders() {
        assert_eq!(detect(&["Sync", "OD", "卒論"]), Some(OneDrive));
        assert_eq!(detect(&["Sync", "OD"]), Some(OneDrive));
        assert_eq!(
            detect(&["Users", "ito", "OneDrive - 研究室", "a"]),
            Some(OneDrive)
        );
        assert_eq!(
            detect(&["Users", "ito", "Dropbox", "work", "proj"]),
            Some(Dropbox)
        );
        assert_eq!(
            detect(&["Users", "ito", "Library", "Mobile Documents", "x", "a"]),
            Some(ICloud)
        );
        assert_eq!(
            detect(&["Users", "ito", "Google Drive", "My Drive", "x"]),
            Some(GoogleDrive)
        );
    }

    #[test]
    fn comparison_ignores_case_dots_and_trailing_separators() {
        assert_eq!(detect(&["sync", "od", "卒論"]), Some(OneDrive));
        assert_eq!(detect(&["SYNC", "Od", "a", "..", "b"]), Some(OneDrive));
        assert_eq!(detect(&["Sync", "OD", ".", "卒論"]), Some(OneDrive));
        assert_eq!(detect(&["users", "ITO", "dropbox", "proj"]), Some(Dropbox));
        let mut trailing = p(&["Sync", "OD", "卒論"]).into_os_string();
        trailing.push(std::path::MAIN_SEPARATOR_STR);
        assert_eq!(
            detect_cloud_sync(Path::new(&trailing), &roots()),
            Some(OneDrive)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_extended_paths_are_compared_like_normal_ones() {
        assert_eq!(
            detect_cloud_sync(Path::new(r"\\?\C:\Sync\OD\卒論"), &roots()),
            Some(OneDrive)
        );
        assert_eq!(
            detect_cloud_sync(Path::new(r"c:\sync\od\卒論\"), &roots()),
            Some(OneDrive)
        );
    }

    #[test]
    fn siblings_and_parents_of_a_base_path_are_not_sync_folders() {
        assert_eq!(detect(&["Sync", "Other", "卒論"]), None);
        assert_eq!(detect(&["Sync"]), None);
        assert_eq!(detect(&["Sync", "ODD", "x"]), None);
        assert_eq!(detect(&["Users", "ito", "Documents", "proj"]), None);
        assert_eq!(detect(&["Users", "ito", "Dropbox2", "proj"]), None);
    }

    #[test]
    fn folder_names_are_recognized_without_base_paths() {
        assert_eq!(
            by_name(&["Users", "a", "OneDrive", "Documents", "卒論"]),
            Some(OneDrive)
        );
        assert_eq!(
            by_name(&["Users", "a", "OneDrive - Contoso", "proj"]),
            Some(OneDrive)
        );
        assert_eq!(
            by_name(&[
                "Users",
                "a",
                "Library",
                "CloudStorage",
                "OneDrive-Personal",
                "p"
            ]),
            Some(OneDrive)
        );
        assert_eq!(
            by_name(&["Users", "a", "Dropbox (Personal)", "proj"]),
            Some(Dropbox)
        );
        assert_eq!(
            by_name(&[
                "Users",
                "a",
                "Library",
                "CloudStorage",
                "GoogleDrive-a@example.com",
                "My Drive",
                "p"
            ]),
            Some(GoogleDrive)
        );
        assert_eq!(by_name(&["My Drive", "proj"]), Some(GoogleDrive));
        assert_eq!(
            by_name(&["Users", "a", "iCloudDrive", "proj"]),
            Some(ICloud)
        );
        assert_eq!(
            by_name(&[
                "Users",
                "a",
                "Library",
                "Mobile Documents",
                "com~apple~CloudDocs",
                "p"
            ]),
            Some(ICloud)
        );
    }

    #[test]
    fn ordinary_folders_are_not_sync_folders() {
        for parts in [
            &["Users", "a", "Documents", "卒論"][..],
            &["work", "OneDriveBackup", "x"],
            &["Projects", "dropbox_export", "x"],
            &["home", "a", "projects", "googledrive-tool"],
            &[],
        ] {
            assert_eq!(by_name(parts), None, "{parts:?}");
        }
    }

    #[test]
    fn a_too_short_base_path_does_not_match_everything() {
        let risky = CloudRoots {
            roots: vec![(OneDrive, p(&[])), (Dropbox, p(&["Users"]))],
        };
        assert_eq!(detect_cloud_sync(&p(&["home", "a", "proj"]), &risky), None);
        // 1 階層の基準（ドライブの直下など）は使える
        assert_eq!(
            detect_cloud_sync(&p(&["Users", "a", "proj"]), &risky),
            Some(Dropbox)
        );
    }

    #[test]
    fn the_real_path_is_checked_too() {
        let none = CloudRoots::default();
        // リンクの先が同期フォルダなら、見かけのパスが普通でも警告する
        let link = p(&["work", "proj"]);
        let real = p(&["Users", "a", "OneDrive", "proj"]);
        assert_eq!(
            detect_cloud_sync_with_real(&link, Some(&real), &none),
            Some(OneDrive)
        );
        assert_eq!(detect_cloud_sync_with_real(&link, None, &none), None);
        assert_eq!(detect_cloud_sync_with_real(&link, Some(&link), &none), None);
    }
}
