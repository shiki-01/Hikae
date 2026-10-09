// 「ディスクの空き容量が足りない」の判定（設計書 5章 E13）。
//
// 書き込みを伴う操作（保存・取り込み・復元点の作成・ファイルの追加）が、空き容量の不足で失敗した
// ことを、git の標準エラー出力の文言と OS のエラーコードの両方から判定する。
// 判定は検出だけで、保存の前に空き容量を調べることはしない（設計書 4.1 に記述が無いため）。

/// git の標準エラー出力が、空き容量の不足を示しているか。
/// 大文字小文字は区別しない。判定に使う文言は git・Windows（msys）・POSIX の英語メッセージ。
pub(crate) fn is_disk_full_message(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();
    const PATTERNS: [&str; 5] = [
        // POSIX の ENOSPC（msys 版 git も同じ文言を出す）
        "no space left on device",
        "enospc",
        // Windows の ERROR_DISK_FULL（112）
        "there is not enough space on the disk",
        "os error 112",
        "disk full",
    ];
    PATTERNS.iter().any(|p| text.contains(p))
}

/// OS のエラーが、空き容量の不足か。
/// Windows は 112（ERROR_DISK_FULL）と 39（ERROR_HANDLE_DISK_FULL）、POSIX は ENOSPC（28）。
pub(crate) fn is_disk_full_error(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::StorageFull {
        return true;
    }
    if cfg!(windows) {
        matches!(e.raw_os_error(), Some(112) | Some(39))
    } else {
        // Linux・macOS ともに ENOSPC は 28
        matches!(e.raw_os_error(), Some(28))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpsError;
    use core_git::GitError;

    #[test]
    fn detects_git_messages_for_a_full_disk() {
        for msg in [
            "error: unable to write file docs/a.docx: No space left on device",
            "fatal: write error: No space left on device",
            "error: unable to write sha1 filename .git/objects/ab/cd: ENOSPC",
            "fatal: failed to write object: There is not enough space on the disk.",
            "fatal: unable to create temporary file: (os error 112)",
            "FATAL: NO SPACE LEFT ON DEVICE",
        ] {
            assert!(is_disk_full_message(msg), "{msg}");
        }
    }

    #[test]
    fn ignores_unrelated_git_messages() {
        for msg in [
            "",
            "fatal: not a git repository",
            "error: unable to unlink old 'a.docx': Permission denied",
            "error: Your local changes would be overwritten by merge",
            "fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host",
            "error: failed to push some refs to 'origin'",
            "remote: error: File big.bin is 120.00 MB; this exceeds GitHub's file size limit",
        ] {
            assert!(!is_disk_full_message(msg), "{msg}");
        }
    }

    #[test]
    fn os_error_codes_are_platform_specific() {
        let enospc = std::io::Error::from_raw_os_error(28);
        let disk_full = std::io::Error::from_raw_os_error(112);
        let handle_disk_full = std::io::Error::from_raw_os_error(39);
        assert_eq!(is_disk_full_error(&enospc), !cfg!(windows));
        assert_eq!(is_disk_full_error(&disk_full), cfg!(windows));
        assert_eq!(is_disk_full_error(&handle_disk_full), cfg!(windows));
        // 無関係なコード（ファイルが無い）は対象外
        assert!(!is_disk_full_error(&std::io::Error::from_raw_os_error(2)));
        // 種類で渡されたものも対象にする
        assert!(is_disk_full_error(&std::io::Error::from(
            std::io::ErrorKind::StorageFull
        )));
    }

    #[test]
    fn git_failure_becomes_disk_full() {
        let e = GitError::Failed {
            code: 128,
            stderr: "error: unable to write file a.docx: No space left on device".to_string(),
            args_summary: "add".to_string(),
        };
        assert!(matches!(OpsError::from(e), OpsError::DiskFull));

        let unrelated = GitError::Failed {
            code: 128,
            stderr: "fatal: not a git repository".to_string(),
            args_summary: "status".to_string(),
        };
        assert!(matches!(OpsError::from(unrelated), OpsError::Git(_)));
    }

    #[test]
    fn disk_full_is_checked_before_file_in_use() {
        // 「unable to write file」は共有違反の文言でもあるが、空き容量の不足を優先する
        let e = GitError::Failed {
            code: 1,
            stderr: "error: unable to write file a.txt: No space left on device (failed)"
                .to_string(),
            args_summary: "restore".to_string(),
        };
        assert!(matches!(OpsError::from(e), OpsError::DiskFull));
    }

    #[test]
    fn safety_failures_become_disk_full() {
        let e = core_safety::SafetyError::Git(GitError::Failed {
            code: 128,
            stderr: "fatal: write error: No space left on device".to_string(),
            args_summary: "write-tree".to_string(),
        });
        assert!(matches!(OpsError::from(e), OpsError::DiskFull));
    }

    #[test]
    fn io_failure_becomes_disk_full_and_does_not_leak() {
        let code = if cfg!(windows) { 112 } else { 28 };
        let e = OpsError::from(std::io::Error::from_raw_os_error(code));
        assert!(matches!(e, OpsError::DiskFull));
        assert_eq!(e.kind(), "disk-full");
    }
}
