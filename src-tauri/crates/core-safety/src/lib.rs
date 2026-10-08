// スナップショット、バックアップ ref、操作ジャーナル、間引き。

mod thin;

pub use thin::{
    plan_thinning, thin_restore_points, ThinPolicy, ThinReport, BACKUP_RETENTION_DAYS,
    DEFAULT_SNAPSHOT_RETENTION_DAYS,
};

use core_git::{GitError, GitRunner};
use std::ffi::OsStr;
use std::path::Path;
use thiserror::Error;
use time::OffsetDateTime;

pub const SNAPSHOT_REF_PREFIX: &str = "refs/hikae/snapshots/";
pub const BACKUP_REF_PREFIX: &str = "refs/hikae/backup/";

/// スナップショット ref と commit の情報
#[derive(Debug, Clone)]
pub struct SnapshotRef {
    pub ref_name: String,
    pub commit: String,
    pub tree: String,
}

/// スナップショット情報（list_snapshots の戻り値用）
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub ref_name: String,
    pub commit: String,
    pub tree: String,
    pub created_at: String, // ref 名の時刻部分（YYYYMMDDTHHMMSSZ）
}

/// 復元点（snapshot と backup ref）
#[derive(Debug, Clone)]
pub struct RestorePoint {
    pub snapshot: Option<SnapshotRef>,
    pub backup_ref: Option<String>,
}

/// 安全性に関連するエラー型
#[derive(Error, Debug)]
pub enum SafetyError {
    #[error("git error: {0}")]
    Git(#[from] GitError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid name: {0}")]
    InvalidName(String),

    #[error("unexpected error: {0}")]
    Unexpected(String),
}

/// branch 名を検証する
fn validate_branch_name(branch: &str) -> Result<(), SafetyError> {
    if branch.is_empty() {
        return Err(SafetyError::InvalidName(
            "branch name cannot be empty".to_string(),
        ));
    }

    if branch.contains("..") {
        return Err(SafetyError::InvalidName(
            "branch name cannot contain '..'".to_string(),
        ));
    }

    if branch.starts_with('/') || branch.ends_with('/') {
        return Err(SafetyError::InvalidName(
            "branch name cannot start or end with '/'".to_string(),
        ));
    }

    // 制御文字・空白・特殊文字をチェック
    for ch in branch.chars() {
        if ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        {
            return Err(SafetyError::InvalidName(format!(
                "branch name contains invalid character: {}",
                ch
            )));
        }
    }

    Ok(())
}

/// operation 名を検証する（[a-z0-9-]+ のみ）
fn validate_operation_name(operation: &str) -> Result<(), SafetyError> {
    if operation.is_empty() {
        return Err(SafetyError::InvalidName(
            "operation name cannot be empty".to_string(),
        ));
    }

    if !operation
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SafetyError::InvalidName(format!(
            "operation name must match [a-z0-9-]+: {}",
            operation
        )));
    }

    Ok(())
}

/// 時刻を ref 名用の形式に変換（YYYYMMDDTHHMMSSZ）
fn format_ref_timestamp(now: OffsetDateTime) -> String {
    // UTC で時刻をフォーマット
    let utc = now.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        utc.year(),
        utc.month() as u8,
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// ref が既に存在するかをチェック
fn ref_exists(runner: &GitRunner, repo: &Path, ref_name: &str) -> Result<bool, SafetyError> {
    let output = runner.run(repo, &["show-ref", "--verify", "-q", ref_name])?;
    Ok(output.code == 0)
}

/// 一意な ref 名を作成する（衝突時は末尾に -1, -2 を付ける）
fn make_unique_ref_name(
    runner: &GitRunner,
    repo: &Path,
    base_ref: &str,
) -> Result<String, SafetyError> {
    if !ref_exists(runner, repo, base_ref)? {
        return Ok(base_ref.to_string());
    }

    // 衝突時は末尾に番号を付ける
    for i in 1..=100 {
        let numbered_ref = format!("{}-{}", base_ref, i);
        if !ref_exists(runner, repo, &numbered_ref)? {
            return Ok(numbered_ref);
        }
    }

    Err(SafetyError::Unexpected(
        "cannot create unique ref name after 100 attempts".to_string(),
    ))
}

/// git の .git ディレクトリパスを取得
fn get_git_dir(runner: &GitRunner, repo: &Path) -> Result<String, SafetyError> {
    let output = runner.run(repo, &["rev-parse", "--git-dir"])?;
    if output.code != 0 {
        return Err(SafetyError::Unexpected(
            format!("failed to get git dir: {}", output.stderr).to_string(),
        ));
    }
    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(git_dir)
}

/// HEAD が存在するかをチェック
fn head_exists(runner: &GitRunner, repo: &Path) -> Result<bool, SafetyError> {
    let output = runner.run(repo, &["rev-parse", "--verify", "HEAD"])?;
    Ok(output.code == 0)
}

/// HEAD の commit ハッシュを取得
fn get_head_commit(runner: &GitRunner, repo: &Path) -> Result<Option<String>, SafetyError> {
    if !head_exists(runner, repo)? {
        return Ok(None);
    }

    let output = runner.run(repo, &["rev-parse", "HEAD"])?;
    if output.code != 0 {
        return Err(SafetyError::Unexpected(
            "failed to get HEAD commit".to_string(),
        ));
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(Some(commit))
}

/// HEAD のツリーハッシュを取得
fn get_head_tree(runner: &GitRunner, repo: &Path) -> Result<Option<String>, SafetyError> {
    if !head_exists(runner, repo)? {
        return Ok(None);
    }

    let output = runner.run(repo, &["rev-parse", "HEAD^{tree}"])?;
    if output.code != 0 {
        return Err(SafetyError::Unexpected(
            "failed to get HEAD tree".to_string(),
        ));
    }

    let tree = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(Some(tree))
}

/// 作業フォルダ全体のスナップショットを作成
pub fn create_snapshot(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    now: OffsetDateTime,
) -> Result<Option<SnapshotRef>, SafetyError> {
    // branch 名を検証
    validate_branch_name(branch)?;

    // 一時インデックスのパスを決定
    let git_dir = get_git_dir(runner, repo)?;
    // git rev-parse --git-dir は相対パスを返すことがあるので、repo に対する相対パスとして結合
    let git_dir_path = if std::path::Path::new(&git_dir).is_absolute() {
        std::path::PathBuf::from(&git_dir)
    } else {
        repo.join(&git_dir)
    };
    let hikae_dir = git_dir_path.join("hikae/tmp");
    std::fs::create_dir_all(&hikae_dir)?;

    let temp_index_name = format!("index-{}", uuid::Uuid::new_v4());
    let temp_index_path = hikae_dir.join(&temp_index_name);
    let temp_index_str = temp_index_path.to_string_lossy();

    // Drop ガード：処理後に一時インデックスを削除
    struct TempIndexGuard {
        path: std::path::PathBuf,
    }
    impl Drop for TempIndexGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
    let _guard = TempIndexGuard {
        path: temp_index_path.clone(),
    };

    // 一時インデックスを初期化
    let head_commit = get_head_commit(runner, repo)?;
    if let Some(commit) = head_commit.as_ref() {
        // HEAD があれば HEAD から read-tree
        runner.run_with_env(
            repo,
            &["read-tree", commit],
            &[("GIT_INDEX_FILE", OsStr::new(&*temp_index_str))],
        )?;
    } else {
        // HEAD がなければ空のインデックスで読み込み
        runner.run_with_env(
            repo,
            &["read-tree", "--empty"],
            &[("GIT_INDEX_FILE", OsStr::new(&*temp_index_str))],
        )?;
    }

    // add -A で作業フォルダをステージング
    runner.run_with_env(
        repo,
        &["add", "-A"],
        &[("GIT_INDEX_FILE", OsStr::new(&*temp_index_str))],
    )?;

    // write-tree で現在のツリーを取得
    let output = runner.run_with_env(
        repo,
        &["write-tree"],
        &[("GIT_INDEX_FILE", OsStr::new(&*temp_index_str))],
    )?;

    if output.code != 0 {
        return Err(SafetyError::Unexpected(format!(
            "write-tree failed: {}",
            output.stderr
        )));
    }

    let new_tree = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // 直前のスナップショットのツリーと比較
    let prev_tree_output = runner.run(
        repo,
        &[
            "for-each-ref",
            "--sort=-refname",
            "--count=1",
            "--format=%(tree)",
            &format!("{}{}/", SNAPSHOT_REF_PREFIX, branch),
        ],
    )?;

    if prev_tree_output.code == 0 {
        let prev_tree = String::from_utf8_lossy(&prev_tree_output.stdout)
            .trim()
            .to_string();
        if !prev_tree.is_empty() && prev_tree == new_tree {
            // ツリーが前のスナップショットと同一 → 重複を作らない
            return Ok(None);
        }
    }

    // HEAD のツリーと比較
    let head_tree = get_head_tree(runner, repo)?;
    if let Some(ht) = head_tree {
        if ht == new_tree {
            // HEAD のツリーと完全一致 → スナップショットを作らない
            return Ok(None);
        }
    }

    // commit-tree で新しい commit を作成
    let timestamp = format_ref_timestamp(now);
    let commit_msg = format!("auto: {}", timestamp);
    let parent_arg;

    let mut args_vec: Vec<&str> = vec![
        "-c",
        "user.name=Hikae",
        "-c",
        "user.email=hikae@localhost",
        "commit-tree",
        &new_tree,
        "-m",
        &commit_msg,
    ];

    if let Some(parent) = head_commit.as_ref() {
        parent_arg = parent.to_string();
        args_vec.push("-p");
        args_vec.push(&parent_arg);
    }

    let output = runner.run_with_env(
        repo,
        &args_vec,
        &[("GIT_INDEX_FILE", OsStr::new(&*temp_index_str))],
    )?;

    if output.code != 0 {
        return Err(SafetyError::Unexpected(format!(
            "commit-tree failed: {}",
            output.stderr
        )));
    }

    let commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // ref 名を生成（衝突対策）
    let base_ref = format!("{}{}{}{}", SNAPSHOT_REF_PREFIX, branch, "/", timestamp);
    let unique_ref_name = make_unique_ref_name(runner, repo, &base_ref)?;

    // update-ref で ref を作成
    runner.run(
        repo,
        &[
            "update-ref",
            &unique_ref_name,
            &commit_hash,
            "-m",
            &format!("snapshot: {}", branch),
        ],
    )?;

    Ok(Some(SnapshotRef {
        ref_name: unique_ref_name,
        commit: commit_hash,
        tree: new_tree,
    }))
}

/// 操作前バックアップ ref を HEAD に対して作成
pub fn create_backup_ref(
    runner: &GitRunner,
    repo: &Path,
    operation: &str,
    now: OffsetDateTime,
) -> Result<Option<String>, SafetyError> {
    // operation 名を検証
    validate_operation_name(operation)?;

    // HEAD がなければ None を返す（不変条件に従う）
    if !head_exists(runner, repo)? {
        return Ok(None);
    }

    let head_commit = get_head_commit(runner, repo)?
        .ok_or_else(|| SafetyError::Unexpected("HEAD exists but commit not found".to_string()))?;

    let timestamp = format_ref_timestamp(now);
    let base_ref = format!("{}{}{}{}", BACKUP_REF_PREFIX, operation, "/", timestamp);

    let unique_ref_name = make_unique_ref_name(runner, repo, &base_ref)?;

    // update-ref で ref を作成
    runner.run(
        repo,
        &[
            "update-ref",
            &unique_ref_name,
            &head_commit,
            "-m",
            &format!("backup: {}", operation),
        ],
    )?;

    Ok(Some(unique_ref_name))
}

/// ブランチのスナップショットを新しい順に列挙
pub fn list_snapshots(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
) -> Result<Vec<SnapshotInfo>, SafetyError> {
    validate_branch_name(branch)?;

    let pattern = format!("{}{}/", SNAPSHOT_REF_PREFIX, branch);
    let output = runner.run(
        repo,
        &[
            "for-each-ref",
            "--sort=-refname",
            "--format=%(refname)%09%(objectname)%09%(tree)",
            &pattern,
        ],
    )?;

    if output.code != 0 {
        return Ok(Vec::new()); // ref がない場合は空リスト
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshots = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let ref_name = parts[0];
            let commit = parts[1];
            let tree = parts[2];

            // ref 名から時刻部分を抽出（refs/hikae/snapshots/<branch>/<時刻>）
            let prefix = format!("{}{}{}", SNAPSHOT_REF_PREFIX, branch, "/");
            let created_at = ref_name.strip_prefix(&prefix).unwrap_or("").to_string();

            snapshots.push(SnapshotInfo {
                ref_name: ref_name.to_string(),
                commit: commit.to_string(),
                tree: tree.to_string(),
                created_at,
            });
        }
    }

    Ok(snapshots)
}

/// 復元点（snapshot + backup ref）を作成
pub fn create_restore_point(
    runner: &GitRunner,
    repo: &Path,
    branch: &str,
    operation: &str,
    now: OffsetDateTime,
) -> Result<RestorePoint, SafetyError> {
    validate_branch_name(branch)?;
    validate_operation_name(operation)?;

    let snapshot = create_snapshot(runner, repo, branch, now)?;
    let backup_ref = create_backup_ref(runner, repo, operation, now)?;

    Ok(RestorePoint {
        snapshot,
        backup_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_branch_name_valid() {
        assert!(validate_branch_name("main").is_ok());
        assert!(validate_branch_name("feature/xyz").is_ok());
        assert!(validate_branch_name("release-1.0").is_ok());
    }

    #[test]
    fn test_validate_branch_name_invalid() {
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("a..b").is_err());
        assert!(validate_branch_name("/main").is_err());
        assert!(validate_branch_name("main/").is_err());
        assert!(validate_branch_name("main\n").is_err());
        assert!(validate_branch_name("main~test").is_err());
    }

    #[test]
    fn test_validate_operation_name() {
        assert!(validate_operation_name("merge").is_ok());
        assert!(validate_operation_name("restore-file").is_ok());
        assert!(validate_operation_name("op-123").is_ok());
        assert!(validate_operation_name("").is_err());
        assert!(validate_operation_name("OP").is_err());
        assert!(validate_operation_name("op_test").is_err());
    }

    #[test]
    fn test_format_ref_timestamp() {
        let dt = time::macros::datetime!(2026-10-07 14:30:45 UTC);
        let formatted = format_ref_timestamp(dt);
        assert_eq!(formatted, "20261007T143045Z");
    }
}
