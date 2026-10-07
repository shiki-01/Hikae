// テスト用のヘルパー関数
#![allow(dead_code)] // テストファイルごとに使う関数が異なるため

use std::path::Path;
use std::process::Command;

/// git コマンドを直接実行（テスト補助用）
pub fn git_command(repo: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to run git command")
}

/// .git/index のバイト列を取得
pub fn read_index_file(repo: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(repo.join(".git/index"))
}

/// 全ファイル（.git 除く）を再帰的に読み込み、ハッシュと mtime を記録
pub fn hash_working_tree(
    repo: &Path,
) -> std::io::Result<std::collections::BTreeMap<String, (String, u64)>> {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use walkdir::WalkDir;

    let mut map = BTreeMap::new();

    for entry in WalkDir::new(repo).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path
            .components()
            .any(|c| c.as_os_str() == std::ffi::OsStr::new(".git"))
        {
            continue;
        }

        if path.is_file() {
            let content = std::fs::read(path)?;
            let metadata = path.metadata()?;

            let mut hasher = Sha256::new();
            hasher.update(&content);
            let hash = format!("{:x}", hasher.finalize());

            let rel_path = path
                .strip_prefix(repo)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let mtime = metadata
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .expect("mtime calculation failed")
                .as_secs();

            map.insert(rel_path, (hash, mtime));
        }
    }

    Ok(map)
}

/// .git/index.lock が存在しないか確認
pub fn check_no_index_lock(repo: &Path) -> bool {
    !repo.join(".git/index.lock").exists()
}

/// 一時インデックスファイルが残っていないか確認
pub fn check_no_temp_index(repo: &Path) -> bool {
    let hikae_dir = repo.join(".git/hikae/tmp");
    if !hikae_dir.exists() {
        return true;
    }

    hikae_dir
        .read_dir()
        .map(|entries| entries.count() == 0)
        .unwrap_or(true)
}
