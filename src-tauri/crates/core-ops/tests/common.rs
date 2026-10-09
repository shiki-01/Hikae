// テスト用のヘルパー関数。実 git を直接呼び出してもよい。

use std::fs;
use std::path::Path;
use std::process::Command;

/// git を直接呼び出す（テスト用ヘルパー）
#[allow(dead_code)]
pub fn run_git(repo: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("failed to execute git");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let code = output.status.code().unwrap_or(-1);

    (code, stdout, stderr)
}

/// 標準入力を渡して git を直接呼び出す（テスト用ヘルパー）
#[allow(dead_code)]
pub fn run_git_stdin(repo: &Path, args: &[&str], input: &str) -> (i32, String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute git");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait git");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// bare リポジトリを作成
#[allow(dead_code)]
pub fn create_bare_repo(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // 親ディレクトリが存在することを確認
    let parent = path.parent().ok_or("no parent dir")?;
    fs::create_dir_all(parent)?;

    // 親ディレクトリから相対パスとして init を実行
    let name = path.file_name().ok_or("no file name")?;
    let name_str = name.to_string_lossy();

    let output = Command::new("git")
        .current_dir(parent)
        .args(["init", "--bare", "-b", "main", &name_str])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("failed to init bare repo: {}", stderr).into());
    }
    Ok(())
}

/// リポジトリの HEAD commit を取得
#[allow(dead_code)]
pub fn get_head_commit(repo: &Path) -> Option<String> {
    let (code, stdout, _) = run_git(repo, &["rev-parse", "HEAD"]);
    if code == 0 {
        Some(stdout.trim().to_string())
    } else {
        None
    }
}

/// リポジトリが bare か確認
#[allow(dead_code)]
pub fn is_bare_repo(repo: &Path) -> bool {
    let (code, stdout, _) = run_git(repo, &["rev-parse", "--is-bare-repository"]);
    code == 0 && stdout.trim() == "true"
}

/// ファイルをリポジトリに追加（テスト用）
#[allow(dead_code)]
pub fn write_test_file(
    repo: &Path,
    rel_path: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let full_path = repo.join(rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(full_path, content)?;
    Ok(())
}

/// merge-base でコミットが祖先か確認（履歴が変更されていないか検証）
#[allow(dead_code)]
pub fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> bool {
    let (code, _, _) = run_git(repo, &["merge-base", "--is-ancestor", ancestor, descendant]);
    code == 0
}

/// refs/hikae/ が remote に存在しないか確認
#[allow(dead_code)]
pub fn has_no_hikae_refs_on_remote(bare_repo: &Path) -> bool {
    let (code, stdout, _) = run_git(bare_repo, &["for-each-ref", "refs/hikae/"]);
    // refs がなければ code=1, output=""
    code != 0 || stdout.trim().is_empty()
}
