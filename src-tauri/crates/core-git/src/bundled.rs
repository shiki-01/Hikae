//! 同梱 git の探索と、子プロセスへ渡す環境変数の組み立て。
//!
//! 探索の優先順位は次のとおり。
//! 1. 環境変数 `HIKAE_GIT_PATH`（開発・テスト用の明示指定。存在するファイルのときだけ採用）
//! 2. 同梱の git（`<resource_dir>/git/` 以下。Windows は MinGit、macOS は自前ビルド）
//! 3. PATH 上の git（開発時のフォールバック）
//!
//! ここで作る環境変数は子プロセスにだけ渡す。ユーザーのグローバル git 設定には書き込まない。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// 明示指定用の環境変数名
pub const GIT_PATH_ENV: &str = "HIKAE_GIT_PATH";

/// 同梱 git のフォルダ構成（OS ごとに異なる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// MinGit: `git/cmd/git.exe`、`git/ucrt64/{bin,libexec/git-core,share/git-core/templates}`、`git/usr/bin`
    Windows,
    /// 自前ビルド: `git/bin/git`、`git/libexec/git-core`、`git/share/git-core/templates`
    Unix,
}

impl Layout {
    /// 実行中の OS に合う構成
    pub fn native() -> Self {
        if cfg!(windows) {
            Layout::Windows
        } else {
            Layout::Unix
        }
    }

    /// 同梱フォルダ（`git/`）からの git 実行ファイルの相対パス
    fn git_exe(self) -> &'static [&'static str] {
        match self {
            Layout::Windows => &["cmd", "git.exe"],
            Layout::Unix => &["bin", "git"],
        }
    }

    /// 同梱フォルダからの exec-path の相対パス
    fn exec_path(self) -> &'static [&'static str] {
        match self {
            Layout::Windows => &["ucrt64", "libexec", "git-core"],
            Layout::Unix => &["libexec", "git-core"],
        }
    }

    /// 同梱フォルダからのテンプレートフォルダの相対パス
    fn template_dir(self) -> &'static [&'static str] {
        match self {
            Layout::Windows => &["ucrt64", "share", "git-core", "templates"],
            Layout::Unix => &["share", "git-core", "templates"],
        }
    }

    /// PATH の先頭に足すフォルダ（同梱フォルダからの相対パス）。
    /// Windows の git-remote-https などは `ucrt64/bin` にあり、`!` 形式の credential helper を
    /// 動かす sh は `usr/bin` にある
    fn path_dirs(self) -> &'static [&'static [&'static str]] {
        match self {
            Layout::Windows => &[&["ucrt64", "bin"], &["usr", "bin"]],
            Layout::Unix => &[&["bin"]],
        }
    }
}

/// 同梱 git を使うときに、子プロセスへ足す設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledEnv {
    /// PATH の先頭に足すフォルダ（存在するものだけ）
    pub path_dirs: Vec<PathBuf>,
    /// `GIT_EXEC_PATH`（存在するときだけ）
    pub exec_path: Option<PathBuf>,
    /// `GIT_TEMPLATE_DIR`（存在するときだけ）
    pub template_dir: Option<PathBuf>,
}

/// 探索の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub git_path: PathBuf,
    pub source: GitSource,
    /// 同梱構成の git を使うときだけ Some
    pub bundled: Option<BundledEnv>,
}

/// git の入手元
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitSource {
    /// `HIKAE_GIT_PATH` で明示指定されたもの
    Explicit,
    /// アプリに同梱されたもの
    Bundled,
    /// PATH 上のもの（開発時のフォールバック）
    Path,
}

fn join(root: &Path, parts: &[&str]) -> PathBuf {
    parts.iter().fold(root.to_path_buf(), |p, s| p.join(s))
}

/// Windows の拡張パス接頭辞（`\\?\`）を外す。git に渡すパスは通常の形式にそろえる。
/// UNC の拡張形式（`\\?\UNC\server\share`）は `\\server\share` に直す。
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        // ドライブ文字付きのときだけ外す（`\\?\Volume{...}` などはそのまま）
        let mut chars = rest.chars();
        if let (Some(d), Some(':')) = (chars.next(), chars.next()) {
            if d.is_ascii_alphabetic() {
                return PathBuf::from(rest);
            }
        }
    }
    path.to_path_buf()
}

/// 同梱フォルダ（`git/`）の構成から、子プロセスに足す設定を作る。
/// 同梱フォルダの形になっていなければ（exec-path が無ければ）None。
fn bundled_env_for_root(root: &Path, layout: Layout) -> Option<BundledEnv> {
    let exec_path = join(root, layout.exec_path());
    // exec-path が無い場合は、同梱構成ではないものとして扱う
    if !exec_path.is_dir() {
        return None;
    }
    let template_dir = join(root, layout.template_dir());
    Some(BundledEnv {
        path_dirs: layout
            .path_dirs()
            .iter()
            .map(|p| join(root, p))
            .filter(|p| p.is_dir())
            .collect(),
        exec_path: Some(exec_path),
        template_dir: template_dir.is_dir().then_some(template_dir),
    })
}

/// git 実行ファイルのパスから同梱構成を推定する（`HIKAE_GIT_PATH` が同梱 git を指すとき用）。
/// 例: `<root>/cmd/git.exe`（Windows）、`<root>/bin/git`（Unix）
pub fn infer_bundled_env(git_path: &Path, layout: Layout) -> Option<BundledEnv> {
    let depth = layout.git_exe().len();
    let mut root = git_path;
    for _ in 0..depth {
        root = root.parent()?;
    }
    // 末尾の構成が一致するときだけ推定する（無関係な git に環境変数を足さない）
    let expected = join(root, layout.git_exe());
    if !same_path(&expected, git_path) {
        return None;
    }
    bundled_env_for_root(root, layout)
}

fn same_path(a: &Path, b: &Path) -> bool {
    // 区切り文字と大文字小文字（Windows）の差を無視して比べる
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// 探索の本体（環境に依存しないよう、引数で受け取る）。
pub fn resolve(resource_dir: Option<&Path>, explicit: Option<&OsStr>, layout: Layout) -> Resolved {
    // (1) 明示指定。空文字や存在しないパスは無視する
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        let path = PathBuf::from(p);
        if path.is_file() {
            let bundled = infer_bundled_env(&path, layout);
            return Resolved {
                git_path: path,
                source: GitSource::Explicit,
                bundled,
            };
        }
    }

    // (2) 同梱の git
    if let Some(dir) = resource_dir {
        let root = strip_verbatim_prefix(dir).join("git");
        let git = join(&root, layout.git_exe());
        if git.is_file() {
            if let Some(env) = bundled_env_for_root(&root, layout) {
                return Resolved {
                    git_path: git,
                    source: GitSource::Bundled,
                    bundled: Some(env),
                };
            }
        }
    }

    // (3) PATH 上の git
    Resolved {
        git_path: PathBuf::from("git"),
        source: GitSource::Path,
        bundled: None,
    }
}

/// 環境変数 `HIKAE_GIT_PATH` だけで決める探索（resource_dir を持たないテスト・開発用）。
pub fn resolve_without_resources(explicit: Option<&OsStr>, layout: Layout) -> Resolved {
    resolve(None, explicit, layout)
}

/// PATH の先頭に `dirs` を足した値を作る。結合できない場合は元の値のまま返す。
pub fn prepend_path(dirs: &[PathBuf], original: Option<OsString>) -> Option<OsString> {
    if dirs.is_empty() {
        return original;
    }
    let existing = original.clone().unwrap_or_default();
    let all = dirs
        .iter()
        .cloned()
        .chain(std::env::split_paths(&existing).filter(|p| !p.as_os_str().is_empty()));
    match std::env::join_paths(all) {
        Ok(joined) => Some(joined),
        Err(_) => original,
    }
}

/// 同梱 git の子プロセスに足す環境変数（PATH 以外）。
pub fn extra_env(env: &BundledEnv) -> Vec<(&'static str, OsString)> {
    let mut vars = Vec::new();
    if let Some(p) = &env.exec_path {
        vars.push(("GIT_EXEC_PATH", p.clone().into_os_string()));
    }
    if let Some(p) = &env.template_dir {
        vars.push(("GIT_TEMPLATE_DIR", p.clone().into_os_string()));
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Windows 構成の偽の同梱フォルダを作る
    fn fake_windows(root: &Path) -> PathBuf {
        let git = root.join("git");
        for d in [
            "cmd",
            "ucrt64/bin",
            "ucrt64/libexec/git-core",
            "ucrt64/share/git-core/templates",
            "usr/bin",
        ] {
            fs::create_dir_all(git.join(d)).unwrap();
        }
        fs::write(git.join("cmd/git.exe"), b"").unwrap();
        git
    }

    fn fake_unix(root: &Path) -> PathBuf {
        let git = root.join("git");
        for d in ["bin", "libexec/git-core", "share/git-core/templates"] {
            fs::create_dir_all(git.join(d)).unwrap();
        }
        fs::write(git.join("bin/git"), b"").unwrap();
        git
    }

    #[test]
    fn explicit_path_wins_over_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        fake_windows(tmp.path());
        let fake = tmp.path().join("my-git.exe");
        fs::write(&fake, b"").unwrap();
        let r = resolve(Some(tmp.path()), Some(fake.as_os_str()), Layout::Windows);
        assert_eq!(r.source, GitSource::Explicit);
        assert_eq!(r.git_path, fake);
        // 同梱構成ではない明示指定には、同梱用の環境変数を足さない
        assert!(r.bundled.is_none());
    }

    #[test]
    fn explicit_path_inside_bundle_infers_environment() {
        let tmp = tempfile::tempdir().unwrap();
        let git = fake_windows(tmp.path());
        let exe = git.join("cmd/git.exe");
        let r = resolve_without_resources(Some(exe.as_os_str()), Layout::Windows);
        assert_eq!(r.source, GitSource::Explicit);
        let env = r.bundled.unwrap();
        assert_eq!(env.exec_path, Some(git.join("ucrt64/libexec/git-core")));
        assert_eq!(
            env.template_dir,
            Some(git.join("ucrt64/share/git-core/templates"))
        );
        assert_eq!(
            env.path_dirs,
            vec![git.join("ucrt64/bin"), git.join("usr/bin")]
        );
    }

    #[test]
    fn bundled_is_used_when_no_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let git = fake_windows(tmp.path());
        let r = resolve(Some(tmp.path()), None, Layout::Windows);
        assert_eq!(r.source, GitSource::Bundled);
        assert_eq!(r.git_path, git.join("cmd").join("git.exe"));
        assert!(r.bundled.is_some());

        let tmp2 = tempfile::tempdir().unwrap();
        let git2 = fake_unix(tmp2.path());
        let r2 = resolve(Some(tmp2.path()), None, Layout::Unix);
        assert_eq!(r2.source, GitSource::Bundled);
        assert_eq!(r2.git_path, git2.join("bin").join("git"));
        assert_eq!(r2.bundled.unwrap().path_dirs, vec![git2.join("bin")]);
    }

    #[test]
    fn falls_back_to_path_when_nothing_found() {
        let tmp = tempfile::tempdir().unwrap();
        // 同梱が無い
        let r = resolve(Some(tmp.path()), None, Layout::Windows);
        assert_eq!(r.source, GitSource::Path);
        assert_eq!(r.git_path, PathBuf::from("git"));
        assert!(r.bundled.is_none());

        // 明示指定が存在しない・空のときは無視して同梱を探す
        let git = fake_unix(tmp.path());
        let missing = tmp.path().join("no-such-git");
        let r = resolve(Some(tmp.path()), Some(missing.as_os_str()), Layout::Unix);
        assert_eq!(r.source, GitSource::Bundled);
        assert_eq!(r.git_path, git.join("bin").join("git"));
        let r = resolve(Some(tmp.path()), Some(OsStr::new("")), Layout::Unix);
        assert_eq!(r.source, GitSource::Bundled);
    }

    #[test]
    fn incomplete_bundle_is_not_used() {
        let tmp = tempfile::tempdir().unwrap();
        // git 実行ファイルだけがあり、exec-path が無い（展開途中など）
        let git = tmp.path().join("git");
        fs::create_dir_all(git.join("cmd")).unwrap();
        fs::write(git.join("cmd/git.exe"), b"").unwrap();
        let r = resolve(Some(tmp.path()), None, Layout::Windows);
        assert_eq!(r.source, GitSource::Path);
    }

    #[test]
    fn extra_env_contains_exec_path_and_template_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let git = fake_unix(tmp.path());
        let env = resolve(Some(tmp.path()), None, Layout::Unix)
            .bundled
            .unwrap();
        let vars = extra_env(&env);
        assert_eq!(
            vars,
            vec![
                (
                    "GIT_EXEC_PATH",
                    git.join("libexec").join("git-core").into_os_string()
                ),
                (
                    "GIT_TEMPLATE_DIR",
                    git.join("share")
                        .join("git-core")
                        .join("templates")
                        .into_os_string()
                ),
            ]
        );
    }

    #[test]
    fn prepend_path_puts_bundled_dirs_first() {
        let a = PathBuf::from("/bundle/bin");
        let b = PathBuf::from("/bundle/usr/bin");
        let original = std::env::join_paths(["/usr/bin", "/bin"]).unwrap();
        let joined = prepend_path(&[a.clone(), b.clone()], Some(original)).unwrap();
        let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(
            parts,
            vec![a, b, PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
        );
        // 元の PATH が無くても作れる
        let only = prepend_path(&[PathBuf::from("/bundle/bin")], None).unwrap();
        assert_eq!(
            std::env::split_paths(&only).collect::<Vec<_>>(),
            vec![PathBuf::from("/bundle/bin")]
        );
        // 足すものが無ければ元のまま
        assert_eq!(prepend_path(&[], None), None);
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\C:\Apps\Hikae")),
            PathBuf::from(r"C:\Apps\Hikae")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\UNC\srv\share\x")),
            PathBuf::from(r"\\srv\share\x")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new("/usr/share/hikae")),
            PathBuf::from("/usr/share/hikae")
        );
    }
}
