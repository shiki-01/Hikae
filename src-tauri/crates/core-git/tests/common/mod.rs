use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// テスト用のリポジトリを一時ディレクトリに作成する
pub fn create_test_repo() -> TempDir {
    let dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = dir.path();

    // git init
    let output = Command::new("git")
        .arg("init")
        .current_dir(repo_path)
        .output()
        .expect("failed to run git init");

    if !output.status.success() {
        panic!(
            "git init failed: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // git config user
    let _ = Command::new("git")
        .arg("-c")
        .arg("user.name=Test")
        .arg("-c")
        .arg("user.email=test@example.com")
        .arg("config")
        .arg("--local")
        .arg("user.name")
        .arg("Test User")
        .current_dir(repo_path)
        .output();

    let _ = Command::new("git")
        .arg("-c")
        .arg("user.name=Test")
        .arg("-c")
        .arg("user.email=test@example.com")
        .arg("config")
        .arg("--local")
        .arg("user.email")
        .arg("test@example.com")
        .current_dir(repo_path)
        .output();

    dir
}

/// テストヘルパー: git コマンドを直接実行（テスト用のブランチ作成等）
pub fn run_git_directly(repo_path: &Path, args: &[&str]) -> bool {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("failed to run git");

    output.status.success()
}
