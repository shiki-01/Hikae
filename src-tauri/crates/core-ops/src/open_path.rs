// 「既定のアプリで開く」対象パスの検証。tauri には依存しない。
// フロントから渡された相対パスが、登録済みプロジェクトの外を指していないことを保証する。

use std::io;
use std::path::{Component, Path, PathBuf};

/// 検証で拒否された理由
#[derive(Debug, thiserror::Error)]
pub enum OpenPathError {
    /// 空、NUL を含むなど、パスとして不正
    #[error("invalid path")]
    Invalid,
    /// 絶対パス・ドライブ指定・UNC・`..` など、プロジェクト外へ出うる指定
    #[error("path is not a plain relative path")]
    NotRelative,
    /// 解決した結果（シンボリックリンク経由を含む）がプロジェクトの外
    #[error("path resolves outside the project")]
    Outside,
    /// 対象が存在しない
    #[error("path does not exist")]
    NotFound,
    /// プロジェクトのルートが解決できない
    #[error("project root is not available: {0}")]
    RootUnavailable(io::Error),
    /// そのほかの I/O エラー
    #[error("io error: {0}")]
    Io(io::Error),
}

/// `root` 配下の `relative` を検証し、正規化した絶対パスを返す。
///
/// 1. 字句検査: 空・NUL・絶対パス・ドライブ指定・UNC・`..`・ルート指定を拒否
/// 2. 実体検査: root と結合後のパスをそれぞれ canonicalize し、後者が前者の配下であることを確認
///    （シンボリックリンクやジャンクションによる脱出はここで拒否される）
pub fn resolve_in_project(root: &Path, relative: &str) -> Result<PathBuf, OpenPathError> {
    if relative.is_empty() || relative.contains('\0') {
        return Err(OpenPathError::Invalid);
    }
    if !is_plain_relative(relative) {
        return Err(OpenPathError::NotRelative);
    }

    let root_real = root
        .canonicalize()
        .map_err(OpenPathError::RootUnavailable)?;
    let joined = root_real.join(relative);
    let target = joined.canonicalize().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            OpenPathError::NotFound
        } else {
            OpenPathError::Io(e)
        }
    })?;

    // Path::starts_with は成分単位で比較するため、"proj" と "proj-evil" は区別される
    if target == root_real || !target.starts_with(&root_real) {
        return Err(OpenPathError::Outside);
    }
    Ok(target)
}

/// 字句だけで見て、プロジェクト内に留まる単純な相対パスか。
fn is_plain_relative(relative: &str) -> bool {
    // OS に依らず、先頭の区切り文字（絶対パス・UNC）とドライブ指定（`C:`）を拒否する
    if relative.starts_with('/') || relative.starts_with('\\') {
        return false;
    }
    let bytes = relative.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    // `\` を区切りとして扱う OS でも、そうでない OS でも `..` を見逃さない
    if relative.split(['/', '\\']).any(|part| part == "..") {
        return false;
    }
    // Windows では `:` が代替データストリームの指定になるため拒否する
    if cfg!(windows) && relative.contains(':') {
        return false;
    }
    let path = Path::new(relative);
    if path.is_absolute() || path.has_root() {
        return false;
    }
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(root.join("docs")).expect("mkdir");
        std::fs::write(root.join("docs").join("a.txt"), "a").expect("write");
        std::fs::write(tmp.path().join("secret.txt"), "s").expect("write");
        tmp
    }

    #[test]
    fn accepts_file_inside_project() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        let got = resolve_in_project(&root, "docs/a.txt").expect("ok");
        assert!(got.ends_with("a.txt"));
        assert!(got.starts_with(root.canonicalize().expect("canon")));
        // ./ を含んでも root 内なら通る
        assert!(resolve_in_project(&root, "./docs/a.txt").is_ok());
    }

    #[test]
    fn rejects_parent_traversal() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        for rel in [
            "../secret.txt",
            "docs/../../secret.txt",
            "docs\\..\\..\\secret.txt",
        ] {
            assert!(
                matches!(
                    resolve_in_project(&root, rel),
                    Err(OpenPathError::NotRelative)
                ),
                "{rel}"
            );
        }
    }

    #[test]
    fn rejects_absolute_drive_and_unc() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        let abs = tmp.path().join("secret.txt");
        let abs = abs.to_string_lossy().into_owned();
        for rel in [
            abs.as_str(),
            "/etc/passwd",
            "C:\\x",
            "C:x",
            "\\\\server\\share\\x",
        ] {
            assert!(
                matches!(
                    resolve_in_project(&root, rel),
                    Err(OpenPathError::NotRelative)
                ),
                "{rel}"
            );
        }
    }

    #[test]
    fn rejects_empty_nul_and_root_itself() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        assert!(matches!(
            resolve_in_project(&root, ""),
            Err(OpenPathError::Invalid)
        ));
        assert!(matches!(
            resolve_in_project(&root, "a\0b"),
            Err(OpenPathError::Invalid)
        ));
        assert!(matches!(
            resolve_in_project(&root, "."),
            Err(OpenPathError::Outside)
        ));
    }

    #[test]
    fn rejects_missing_path() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        assert!(matches!(
            resolve_in_project(&root, "docs/none.txt"),
            Err(OpenPathError::NotFound)
        ));
    }

    #[test]
    fn rejects_missing_root() {
        let tmp = setup();
        let root = tmp.path().join("no-such-project");
        assert!(matches!(
            resolve_in_project(&root, "a.txt"),
            Err(OpenPathError::RootUnavailable(_))
        ));
    }

    /// シンボリックリンクを作れない環境（Windows の権限なし等）では None を返す
    fn try_symlink(target: &Path, link: &Path) -> Option<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).ok()
        }
        #[cfg(windows)]
        {
            if target.is_dir() {
                std::os::windows::fs::symlink_dir(target, link).ok()
            } else {
                std::os::windows::fs::symlink_file(target, link).ok()
            }
        }
    }

    #[test]
    fn rejects_symlink_escaping_project() {
        let tmp = setup();
        let root = tmp.path().join("proj");
        let outside_file = tmp.path().join("secret.txt");
        if try_symlink(&outside_file, &root.join("link.txt")).is_none() {
            eprintln!("skip: symlink を作成できない環境");
            return;
        }
        assert!(matches!(
            resolve_in_project(&root, "link.txt"),
            Err(OpenPathError::Outside)
        ));

        // ディレクトリのシンボリックリンク経由でも脱出できない
        if try_symlink(tmp.path(), &root.join("up")).is_some() {
            assert!(matches!(
                resolve_in_project(&root, "up/secret.txt"),
                Err(OpenPathError::Outside)
            ));
        }
    }

    #[test]
    fn rejects_sibling_with_same_prefix() {
        // "proj" と "proj-evil" を文字列の前方一致で混同しない
        let tmp = setup();
        let root = tmp.path().join("proj");
        let evil = tmp.path().join("proj-evil");
        std::fs::create_dir_all(&evil).expect("mkdir");
        std::fs::write(evil.join("x.txt"), "x").expect("write");
        if try_symlink(&evil, &root.join("evil")).is_none() {
            eprintln!("skip: symlink を作成できない環境");
            return;
        }
        assert!(matches!(
            resolve_in_project(&root, "evil/x.txt"),
            Err(OpenPathError::Outside)
        ));
    }
}
