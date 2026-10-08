// ファイル監視で無視するパスの判定（設計書 7章「自動保存」、13.1「ファイル監視の負荷」）。
//
// 無視するもの:
// - `.git` 配下（アプリ自身の一時インデックスや ref の更新を拾うと、自動保存が自分で自分を呼ぶため）
// - OS・Office の一時ファイル（`~$*`、`.DS_Store`、`Thumbs.db` など）
// - プロジェクト直下の `.gitignore` と `.git/info/exclude` に書かれた保存対象外のパス
//
// 判定は純粋な関数で、ファイルシステムの状態には依存しない（ルールの読み込み `load` を除く）。
// サブフォルダの `.gitignore` は読まない。読まない場合は「無視しない」側に倒れるだけで、
// 余分な通知が増えるが、変更の見落としにはならない。

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Component, Path, PathBuf};

/// ファイル名が完全に一致したら無視する名前（OS が作るメタデータ）
const IGNORED_FILE_NAMES: &[&str] = &[".DS_Store", "Thumbs.db", "ehthumbs.db", "desktop.ini"];

/// 設計書にある除外のうち、パスだけで決まるもの（`.gitignore` には依存しない）。
/// `rel` はプロジェクトからの相対パス。
pub fn is_builtin_ignored(rel: &Path) -> bool {
    for component in rel.components() {
        if let Component::Normal(name) = component {
            if name == ".git" {
                return true;
            }
        }
    }
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    // Office のロックファイル（`~$報告書.docx`）、Word の一時ファイル（`~WRL0001.tmp`）
    if name.starts_with("~$") || (name.starts_with('~') && name.ends_with(".tmp")) {
        return true;
    }
    // LibreOffice のロックファイル（`.~lock.報告書.odt#`）
    if name.starts_with(".~lock.") {
        return true;
    }
    IGNORED_FILE_NAMES.contains(&name)
}

/// 無視するパスの規則（設計書で決めた除外 + プロジェクトの `.gitignore`）
#[derive(Debug, Clone)]
pub struct IgnoreRules {
    gitignore: Gitignore,
}

impl IgnoreRules {
    /// 組み込みの除外だけの規則（`.gitignore` は無い）
    pub fn builtin_only() -> Self {
        IgnoreRules {
            gitignore: Gitignore::empty(),
        }
    }

    /// プロジェクト直下の `.gitignore` と `.git/info/exclude` を読む。
    /// 読めない、書式が正しくない行がある場合は、読めた分だけを使う（監視を止めない）。
    pub fn load(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        // Windows と macOS のファイルシステムは大文字小文字を区別しないため、git も区別しない
        let _ = builder.case_insensitive(cfg!(any(windows, target_os = "macos")));
        // 無い場合のエラーは無視する（ルールが無いだけ）
        let _ = builder.add(root.join(".git").join("info").join("exclude"));
        let _ = builder.add(root.join(".gitignore"));
        let gitignore = builder.build().unwrap_or_else(|_| Gitignore::empty());
        IgnoreRules { gitignore }
    }

    /// 行のリストから作る（テスト用）。`root` はプロジェクトのフォルダ
    pub fn from_lines(root: &Path, lines: &[&str]) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        for line in lines {
            let _ = builder.add_line(None, line);
        }
        let gitignore = builder.build().unwrap_or_else(|_| Gitignore::empty());
        IgnoreRules { gitignore }
    }

    /// プロジェクトからの相対パスが無視対象か。`is_dir` はフォルダ自体の変更かどうか
    /// （削除済みで分からないときは false でよい。親フォルダは常にフォルダとして調べる）。
    pub fn is_ignored(&self, rel: &Path, is_dir: bool) -> bool {
        if rel.as_os_str().is_empty() {
            return false;
        }
        if is_builtin_ignored(rel) {
            return true;
        }
        // ルートの外を指す相対パスは判定できない（無視しない側に倒す）
        if rel.has_root() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
            return false;
        }
        self.gitignore
            .matched_path_or_any_parents(rel, is_dir)
            .is_ignore()
    }
}

/// `path` をプロジェクトからの相対パスにする。ルートの外、または判定できないときは None。
/// `roots` は、与えられたフォルダ名と正規化したフォルダ名（macOS の `/var` → `/private/var` など）。
pub fn relative_to_roots(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(lines: &[&str]) -> IgnoreRules {
        IgnoreRules::from_lines(Path::new("/proj"), lines)
    }

    #[test]
    fn dot_git_is_always_ignored() {
        let r = IgnoreRules::builtin_only();
        assert!(r.is_ignored(Path::new(".git"), true));
        assert!(r.is_ignored(Path::new(".git/index.lock"), false));
        assert!(r.is_ignored(Path::new(".git/hikae/tmp/index-1"), false));
        // 名前が似ているだけのものは無視しない
        assert!(!r.is_ignored(Path::new(".github/workflows/ci.yml"), false));
        assert!(!r.is_ignored(Path::new("memo.git"), false));
    }

    #[test]
    fn os_and_office_temporary_files_are_ignored() {
        let r = IgnoreRules::builtin_only();
        for name in [
            "~$報告書.docx",
            "sub/~$表.xlsx",
            "~WRL0001.tmp",
            ".DS_Store",
            "a/b/.DS_Store",
            "Thumbs.db",
            "desktop.ini",
            ".~lock.報告書.odt#",
        ] {
            assert!(r.is_ignored(Path::new(name), false), "{name}");
        }
        for name in [
            "報告書.docx",
            "a~b.txt",
            "tilde~.tmp",
            "Thumbs.db.bak",
            "x.DS_Store",
        ] {
            assert!(!r.is_ignored(Path::new(name), false), "{name}");
        }
    }

    #[test]
    fn gitignore_patterns_are_applied() {
        let r = rules(&["*.log", "build/", "/secret.txt", "!keep.log"]);
        assert!(r.is_ignored(Path::new("a/b/debug.log"), false));
        assert!(!r.is_ignored(Path::new("keep.log"), false));
        assert!(r.is_ignored(Path::new("build"), true));
        // 削除済みで種別が分からない子の変更も、親フォルダで判定する
        assert!(r.is_ignored(Path::new("build/out/app.exe"), false));
        assert!(r.is_ignored(Path::new("secret.txt"), false));
        assert!(!r.is_ignored(Path::new("sub/secret.txt"), false));
        assert!(!r.is_ignored(Path::new("memo.txt"), false));
    }

    #[test]
    fn paths_outside_the_project_are_not_judged() {
        let r = rules(&["*"]);
        assert!(!r.is_ignored(Path::new("../outside.txt"), false));
        assert!(!r.is_ignored(Path::new(""), false));
    }

    #[test]
    fn relative_path_uses_the_first_matching_root() {
        let roots = vec![PathBuf::from("/var/p"), PathBuf::from("/private/var/p")];
        assert_eq!(
            relative_to_roots(Path::new("/private/var/p/a/b.txt"), &roots),
            Some(PathBuf::from("a/b.txt"))
        );
        assert_eq!(
            relative_to_roots(Path::new("/var/p/c.txt"), &roots),
            Some(PathBuf::from("c.txt"))
        );
        assert_eq!(relative_to_roots(Path::new("/other/x"), &roots), None);
    }

    #[test]
    fn load_reads_gitignore_and_exclude_from_the_project_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git/info")).expect("mkdir");
        std::fs::write(root.join(".gitignore"), "*.bak\n").expect("write");
        std::fs::write(root.join(".git/info/exclude"), "local-only/\n").expect("write");
        let r = IgnoreRules::load(root);
        assert!(r.is_ignored(Path::new("old.bak"), false));
        assert!(r.is_ignored(Path::new("local-only/x.txt"), false));
        assert!(!r.is_ignored(Path::new("new.txt"), false));
        // .gitignore が無い・読めないときは、組み込みの除外だけ
        let empty = tempfile::tempdir().expect("tempdir");
        let r = IgnoreRules::load(empty.path());
        assert!(!r.is_ignored(Path::new("old.bak"), false));
        assert!(r.is_ignored(Path::new(".git/HEAD"), false));
    }
}
