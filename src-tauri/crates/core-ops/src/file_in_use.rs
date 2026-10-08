// 「ファイルが他のアプリで使用中」の判定（設計書 5章 E12）。
//
// Windows では、Word などで開いているファイルを git が上書き・削除しようとすると共有違反になる。
// git の標準エラー出力の文言と、OS のエラーコードの両方から判定する。

/// git の標準エラー出力が、他のアプリによるファイル使用中を示しているか。
/// 大文字小文字は区別しない。判定に使う文言は git と Windows の英語メッセージ。
pub(crate) fn is_file_in_use_message(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();

    // OS が共有違反を直接伝える文言
    const DIRECT: [&str; 3] = [
        "being used by another process",
        "sharing violation",
        "device or resource busy",
    ];
    if DIRECT.iter().any(|p| text.contains(p)) {
        return true;
    }

    // ファイルの削除・作成に失敗したことを示す git の文言
    const FILE_OPERATION: [&str; 6] = [
        "unable to unlink",
        "unlink of file",
        "could not unlink",
        "cannot unlink",
        "unable to create file",
        "unable to write file",
    ];
    let failed_file_operation = FILE_OPERATION.iter().any(|p| text.contains(p));
    // 共有違反は Windows の git では Permission denied や Invalid argument になる（実測）。
    // ファイル操作の失敗を示す文言と、その理由が揃った場合に限って採用する
    const REASONS: [&str; 5] = [
        "permission denied",
        "invalid argument",
        "unlink of file",
        "busy",
        "failed",
    ];
    failed_file_operation && REASONS.iter().any(|r| text.contains(r))
}

/// OS のエラーコードが共有違反（32）またはロック違反（33）か。Windows のみで判定する。
pub(crate) fn is_sharing_violation(e: &std::io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(32) | Some(33))
}

/// 標準エラー出力から、使用中のファイル名（パスは含めない）を取り出す。
/// `unable to unlink old 'dir/第3章.docx': Permission denied` と
/// `unable to create file dir/第3章.docx: Permission denied` の形式に対応する。
pub(crate) fn file_name_in_message(stderr: &str) -> Option<String> {
    let path = quoted_path(stderr).or_else(|| path_after_create_file(stderr))?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 最初のシングルクォートで囲まれた文字列
fn quoted_path(text: &str) -> Option<String> {
    let start = text.find('\'')? + 1;
    let len = text[start..].find('\'')?;
    let quoted = &text[start..start + len];
    if quoted.is_empty() {
        None
    } else {
        Some(quoted.to_string())
    }
}

/// `unable to create file <path>: <理由>` の `<path>`
fn path_after_create_file(text: &str) -> Option<String> {
    const KEY: &str = "unable to create file ";
    let lower = text.to_ascii_lowercase();
    // 小文字化しても ASCII のキーの位置は元の文字列と同じバイト位置になる
    let start = lower.find(KEY)? + KEY.len();
    let rest = text.get(start..)?;
    let end = rest.find(": ").unwrap_or(rest.len());
    let path = rest[..end].lines().next()?.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpsError;
    use core_git::GitError;

    #[test]
    fn detects_git_messages_for_files_in_use() {
        for msg in [
            "error: unable to unlink old 'docs/第3章.docx': Permission denied",
            "error: unable to unlink old '第3章.docx': Invalid argument",
            "error: unable to create file docs/第3章.docx: Permission denied",
            "warning: Unlink of file 'a.docx' failed. Should I try again? (y/n)",
            "error: could not unlink 'a.xlsx': Permission denied",
            "fatal: cannot unlink 'a.xlsx': Permission denied",
            "error: unable to write file a.txt: Device or resource busy",
            "error: open(\"a.docx\"): The process cannot access the file because it is being used by another process.",
            "error: sharing violation on a.docx",
            "ERROR: UNABLE TO UNLINK OLD 'A.DOCX': PERMISSION DENIED",
        ] {
            assert!(is_file_in_use_message(msg), "{msg}");
        }
    }

    #[test]
    fn ignores_unrelated_git_messages() {
        for msg in [
            "",
            "fatal: not a git repository",
            "error: Your local changes would be overwritten by merge",
            "fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host",
            "remote: Permission denied to user",
            "fatal: Authentication failed for 'https://github.com/a/b.git/'",
            "error: failed to push some refs to 'origin'",
        ] {
            assert!(!is_file_in_use_message(msg), "{msg}");
        }
    }

    #[test]
    fn extracts_only_the_file_name() {
        assert_eq!(
            file_name_in_message(
                "error: unable to unlink old '資料/第3章.docx': Permission denied"
            )
            .as_deref(),
            Some("第3章.docx")
        );
        assert_eq!(
            file_name_in_message(
                "error: unable to create file a b\\報告 書.xlsx: Permission denied"
            )
            .as_deref(),
            Some("報告 書.xlsx")
        );
        assert_eq!(file_name_in_message("sharing violation"), None);
        assert_eq!(
            file_name_in_message("error: unable to unlink old '': x"),
            None
        );
    }

    #[test]
    fn sharing_violation_codes_only_count_on_windows() {
        let e = std::io::Error::from_raw_os_error(32);
        assert_eq!(is_sharing_violation(&e), cfg!(windows));
        let other = std::io::Error::from_raw_os_error(2);
        assert!(!is_sharing_violation(&other));
    }

    #[test]
    fn git_failure_becomes_file_in_use() {
        let e = GitError::Failed {
            code: 1,
            stderr: "error: unable to unlink old '資料/第3章.docx': Permission denied".to_string(),
            args_summary: "restore".to_string(),
        };
        match OpsError::from(e) {
            OpsError::FileInUse { file } => assert_eq!(file.as_deref(), Some("第3章.docx")),
            other => panic!("expected FileInUse, got {other:?}"),
        }

        let unrelated = GitError::Failed {
            code: 128,
            stderr: "fatal: not a git repository".to_string(),
            args_summary: "status".to_string(),
        };
        assert!(matches!(OpsError::from(unrelated), OpsError::Git(_)));
    }

    #[test]
    fn safety_git_failure_becomes_file_in_use() {
        let e = core_safety::SafetyError::Git(GitError::Failed {
            code: 1,
            stderr: "error: unable to unlink old 'a.docx': Permission denied".to_string(),
            args_summary: "add".to_string(),
        });
        assert!(matches!(
            OpsError::from(e),
            OpsError::FileInUse { file: Some(_) }
        ));
    }

    #[test]
    fn file_in_use_kind_does_not_leak_file_name() {
        let e = OpsError::FileInUse {
            file: Some("secret.docx".to_string()),
        };
        assert_eq!(e.kind(), "file-in-use");
    }
}
