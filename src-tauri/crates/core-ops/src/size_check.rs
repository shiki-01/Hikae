// 保存前のサイズ検査（設計書 4.1 手順 1、5章 E07 / E08、7章「大きいファイルの警告閾値」）。
//
// - 新規・変更されたファイルだけを対象にする（`status --porcelain=v2 -z` と作業フォルダのメタデータ）
// - 警告閾値（既定 50MB）を超えるものは「警告」（保存はできる）、100MB を超えるものは「保存不可」
// - 結果は `AppError` ではなく構造化データ（どのファイルが何バイトか）で返す。
//   画面は E07 / E08 の 2 択ダイアログを出し、選択を `SaveOptions` に載せて保存をやり直す
// - 「外す」を選んだファイルは、保存の直前（復元点を作った後）に .gitignore へ追記し、
//   追跡中なら `rm --cached` で保存対象から外す。作業フォルダのファイル自体は消さない（不変条件 6）

use crate::add_files::{LARGE_FILE_LIMIT_BYTES, LARGE_FILE_WARN_BYTES};
use crate::models::OpsError;
use core_git::{GitRunner, StatusKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

const BYTES_PER_MB: u64 = 1024 * 1024;

/// 検査の閾値
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeLimits {
    /// これを超えるファイルは警告する
    pub warn_bytes: u64,
    /// これを超えるファイルは保存できない（GitHub の上限）
    pub block_bytes: u64,
}

impl Default for SizeLimits {
    fn default() -> Self {
        SizeLimits {
            warn_bytes: LARGE_FILE_WARN_BYTES,
            block_bytes: LARGE_FILE_LIMIT_BYTES,
        }
    }
}

impl SizeLimits {
    /// 設定「大きいファイルの警告閾値（MB）」から作る。保存不可の上限は固定（100MB）。
    /// 警告閾値が上限を超えていても、上限までに丸める。
    pub fn from_warn_mb(mb: u32) -> Self {
        let limits = SizeLimits::default();
        SizeLimits {
            warn_bytes: (u64::from(mb) * BYTES_PER_MB).min(limits.block_bytes),
            block_bytes: limits.block_bytes,
        }
    }
}

/// 大きいファイル 1 件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LargeFile {
    /// プロジェクトからの相対パス（`/` 区切り）
    pub path: String,
    /// ファイルサイズ（バイト）
    pub size: u64,
}

/// 検査の結果
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SizeFindings {
    /// 保存できない大きさのファイル（E07）
    pub blocked: Vec<LargeFile>,
    /// 警告が必要な大きさのファイル（E08）
    pub warned: Vec<LargeFile>,
}

impl SizeFindings {
    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty() && self.warned.is_empty()
    }
}

/// 保存時の選択
#[derive(Debug, Clone, Default)]
pub struct SaveOptions {
    pub limits: SizeLimits,
    /// 警告（E08）のファイルをそのまま保存することを承諾した
    pub accept_warned: bool,
    /// 保存対象から外すファイル。検査で見つかった大きいファイルのパスだけを指定できる
    pub exclude: Vec<String>,
}

/// サイズからファイルを分類する（純関数）。サイズが閾値ちょうどのものは対象外（「超」で判定）。
pub fn classify_sizes(files: &[(String, u64)], limits: SizeLimits) -> SizeFindings {
    let mut findings = SizeFindings::default();
    for (path, size) in files {
        let file = LargeFile {
            path: path.clone(),
            size: *size,
        };
        if *size > limits.block_bytes {
            findings.blocked.push(file);
        } else if *size > limits.warn_bytes {
            findings.warned.push(file);
        }
    }
    findings.blocked.sort_by(|a, b| a.path.cmp(&b.path));
    findings.warned.sort_by(|a, b| a.path.cmp(&b.path));
    findings
}

/// 新規・変更されたファイルのサイズを調べて分類する（読み取りのみ）。
pub(crate) fn scan(
    runner: &GitRunner,
    repo: &Path,
    limits: SizeLimits,
) -> Result<SizeFindings, OpsError> {
    // 未追跡のフォルダを 1 件にまとめず、中のファイルを 1 件ずつ列挙する
    let out = runner.run_ok(
        repo,
        &["status", "--porcelain=v2", "-z", "-uall", "--no-renames"],
    )?;
    let status =
        core_git::parse_status_v2(&out.stdout).map_err(|e| OpsError::Unexpected(e.to_string()))?;

    let mut sizes = Vec::new();
    for entry in &status.entries {
        match entry.kind {
            // 保存の対象にならないもの、保存前に解決が必要なものは対象外
            StatusKind::Ignored | StatusKind::Unmerged { .. } => continue,
            StatusKind::Change { .. } | StatusKind::Rename { .. } | StatusKind::Untracked => {}
        }
        // 削除されたファイル、フォルダ、リンクはサイズの問題にならない
        let Ok(meta) = std::fs::symlink_metadata(repo.join(&entry.path)) else {
            continue;
        };
        if meta.is_file() {
            sizes.push((entry.path.clone(), meta.len()));
        }
    }
    Ok(classify_sizes(&sizes, limits))
}

/// `exclude` が検査で見つかったファイルだけを指していることを確認し、正規化したパスを返す。
pub(crate) fn validate_exclusions(
    exclude: &[String],
    findings: &SizeFindings,
) -> Result<Vec<String>, OpsError> {
    let mut normalized = Vec::new();
    for raw in exclude {
        let rel = crate::restore_file::normalize_project_path(raw)?;
        if rel.contains(['\n', '\r']) {
            return Err(OpsError::InvalidInput(
                "file name cannot be excluded".to_string(),
            ));
        }
        let flagged = findings
            .blocked
            .iter()
            .chain(&findings.warned)
            .any(|f| f.path == rel);
        if !flagged {
            return Err(OpsError::InvalidInput(format!(
                "'{rel}' is not a large file that can be excluded"
            )));
        }
        if !normalized.contains(&rel) {
            normalized.push(rel);
        }
    }
    Ok(normalized)
}

/// 決定が必要なファイル（外すと選んだものを除いた、保存不可と未承諾の警告）を返す。
/// 空なら、そのまま保存してよい。
pub(crate) fn unresolved(
    findings: &SizeFindings,
    exclude: &[String],
    accept_warned: bool,
) -> SizeFindings {
    let remaining = |files: &[LargeFile]| -> Vec<LargeFile> {
        files
            .iter()
            .filter(|f| !exclude.contains(&f.path))
            .cloned()
            .collect()
    };
    SizeFindings {
        blocked: remaining(&findings.blocked),
        warned: if accept_warned {
            Vec::new()
        } else {
            remaining(&findings.warned)
        },
    }
}

/// .gitignore に書くパターン（ルート直下からの完全一致）にする。
/// git が特別扱いする文字と、末尾の空白をエスケープする。
pub(crate) fn ignore_pattern(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 1);
    out.push('/');
    let trailing_spaces = path.len() - path.trim_end_matches(' ').len();
    let body_len = path.len() - trailing_spaces;
    for (i, ch) in path.char_indices() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']') || (ch == ' ' && i >= body_len) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// .gitignore へパターンを追記する（既にある行は重複して書かない）。
pub(crate) fn append_ignore_patterns(repo: &Path, paths: &[String]) -> Result<(), OpsError> {
    use std::io::Write;

    let file = repo.join(".gitignore");
    let existing = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let text = String::from_utf8_lossy(&existing);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let known: Vec<&str> = text.lines().collect();

    let mut add = String::new();
    if !existing.is_empty() && !existing.ends_with(b"\n") {
        add.push_str(eol);
    }
    let mut wrote_any = false;
    for path in paths {
        let pattern = ignore_pattern(path);
        if known.contains(&pattern.as_str()) {
            continue;
        }
        add.push_str(&pattern);
        add.push_str(eol);
        wrote_any = true;
    }
    if !wrote_any {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    f.write_all(add.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mb(n: u64) -> u64 {
        n * BYTES_PER_MB
    }

    #[test]
    fn classifies_by_strictly_greater_than() {
        let limits = SizeLimits::default();
        let files = vec![
            ("small.txt".to_string(), 10),
            ("at-warn.bin".to_string(), mb(50)),
            ("warn.psd".to_string(), mb(50) + 1),
            ("at-block.bin".to_string(), mb(100)),
            ("動画.mp4".to_string(), mb(250)),
        ];
        let f = classify_sizes(&files, limits);
        assert_eq!(
            f.warned.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(),
            vec!["at-block.bin", "warn.psd"]
        );
        assert_eq!(
            f.blocked,
            vec![LargeFile {
                path: "動画.mp4".to_string(),
                size: mb(250)
            }]
        );
    }

    #[test]
    fn warn_threshold_follows_setting_and_is_capped_at_the_hard_limit() {
        assert_eq!(SizeLimits::from_warn_mb(25).warn_bytes, mb(25));
        assert_eq!(SizeLimits::from_warn_mb(100).warn_bytes, mb(100));
        assert_eq!(SizeLimits::from_warn_mb(500).warn_bytes, mb(100));
        assert_eq!(SizeLimits::from_warn_mb(25).block_bytes, mb(100));
        // 警告閾値が 100MB のとき、警告の区間は空になる（超えたものはすべて保存不可）
        let f = classify_sizes(
            &[("a".to_string(), mb(100) + 1)],
            SizeLimits::from_warn_mb(100),
        );
        assert!(f.warned.is_empty() && f.blocked.len() == 1);
    }

    #[test]
    fn unresolved_excludes_chosen_files_and_accepted_warnings() {
        let findings = SizeFindings {
            blocked: vec![LargeFile {
                path: "big".to_string(),
                size: 9,
            }],
            warned: vec![
                LargeFile {
                    path: "mid1".to_string(),
                    size: 5,
                },
                LargeFile {
                    path: "mid2".to_string(),
                    size: 5,
                },
            ],
        };
        assert_eq!(unresolved(&findings, &[], false), findings);
        let u = unresolved(&findings, &["big".to_string(), "mid1".to_string()], false);
        assert!(u.blocked.is_empty());
        assert_eq!(u.warned.len(), 1);
        let u = unresolved(&findings, &[], true);
        assert_eq!(u.blocked.len(), 1);
        assert!(u.warned.is_empty());
        assert!(unresolved(&findings, &["big".to_string()], true).is_empty());
    }

    #[test]
    fn ignore_pattern_escapes_special_characters() {
        assert_eq!(ignore_pattern("a/b.mp4"), "/a/b.mp4");
        assert_eq!(ignore_pattern("動画 1.mp4"), "/動画 1.mp4");
        assert_eq!(ignore_pattern("#x*[1]?.bin"), "/#x\\*\\[1\\]\\?.bin");
        assert_eq!(ignore_pattern("a\\b"), "/a\\\\b");
        assert_eq!(ignore_pattern("end  "), "/end\\ \\ ");
        assert_eq!(ignore_pattern("in side"), "/in side");
    }

    #[test]
    fn append_ignore_patterns_keeps_existing_content_and_skips_duplicates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        std::fs::write(repo.join(".gitignore"), "*.log").expect("write");
        append_ignore_patterns(repo, &["a/big.bin".to_string(), "b.psd".to_string()])
            .expect("append");
        append_ignore_patterns(repo, &["a/big.bin".to_string()]).expect("append again");
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).expect("read"),
            "*.log\n/a/big.bin\n/b.psd\n"
        );

        // CRLF の .gitignore は CRLF のまま追記する
        std::fs::write(repo.join(".gitignore"), "x\r\n").expect("write");
        append_ignore_patterns(repo, &["y".to_string()]).expect("append");
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).expect("read"),
            "x\r\n/y\r\n"
        );

        // 無ければ作る
        std::fs::remove_file(repo.join(".gitignore")).expect("rm");
        append_ignore_patterns(repo, &["z".to_string()]).expect("append");
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).expect("read"),
            "/z\n"
        );
    }
}
