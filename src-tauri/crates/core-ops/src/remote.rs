// クラウドの保管場所（remote）の接続と、同期状態の表示用データ（設計書 4.6、9.2）。
//
// - 接続は `origin` を 1 つ設定するだけ（`.git/config` のみ変更し、作業フォルダ・インデックスは変えない）。
//   URL に認証情報を含めない（不変条件 9。認証は credential helper が行う）
// - 履歴の「ここまでクラウド」と最終アップロード日時は、読み取りだけで求める
// - フォルダの存在確認（E11）は git を実行せず、ファイルシステムだけで行う

use crate::models::*;
use crate::operations::save;
use crate::pc_name::Meta;
use crate::relocate::normalize_remote_url;
use crate::size_check::{SaveOptions, SizeFindings, SizeLimits};
use core_git::GitRunner;
use std::collections::HashSet;
use std::path::Path;
use time::OffsetDateTime;

/// 登録されたプロジェクトのフォルダの状態（設計書 5章 E11）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    /// 使える（フォルダがあり、リポジトリである）
    Available,
    /// 存在しない（移動・名前変更・削除、または読めない）
    Missing,
    /// 存在するがフォルダではない
    NotADirectory,
    /// フォルダはあるが、リポジトリではない
    NotARepository,
}

impl FolderState {
    /// 画面の「フォルダが見つかりません」（E11）として扱う状態か
    pub fn is_missing(self) -> bool {
        self != FolderState::Available
    }
}

/// 登録パスの状態を調べる。git は実行しない（フォルダが無いと git はエラーになるため）。
pub fn project_folder_state(path: &Path) -> FolderState {
    match std::fs::metadata(path) {
        Err(_) => FolderState::Missing,
        Ok(meta) if !meta.is_dir() => FolderState::NotADirectory,
        // 通常は `.git` フォルダ。作業ツリーなどでは `.git` がファイルのこともある
        Ok(_) if path.join(".git").exists() => FolderState::Available,
        Ok(_) => FolderState::NotARepository,
    }
}

/// `origin` の接続結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteConnection {
    /// 新しく設定した
    Added,
    /// すでに同じ場所が設定されていた（再試行しても同じ結果になる）
    AlreadyConnected,
}

/// 接続先 URL を検証する。空、先頭が `-`（オプションとして解釈される）、空白・制御文字を含むもの、
/// URL に認証情報（`user:pass@`）が含まれるものは受け付けない。
fn validate_remote_url(url: &str) -> Result<(), OpsError> {
    let bad = |why: &str| OpsError::InvalidInput(format!("remote url is not usable: {why}"));
    if url.is_empty() {
        return Err(bad("empty"));
    }
    if url.starts_with('-') {
        return Err(bad("starts with '-'"));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("contains whitespace or control characters"));
    }
    if let Some((_, rest)) = url.split_once("://") {
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.contains('@') {
            return Err(bad("contains credentials"));
        }
    }
    Ok(())
}

/// `origin` を設定する。すでに同じ場所が設定されていれば何もしない。
/// 別の場所が設定されている場合は上書きせず拒否する（誤って付け替えないため）。
pub(crate) fn connect_remote(
    runner: &GitRunner,
    repo: &Path,
    url: &str,
) -> Result<RemoteConnection, OpsError> {
    validate_remote_url(url)?;
    let current = runner.run(repo, &["remote", "get-url", "origin"])?;
    if current.code == 0 {
        let existing = String::from_utf8_lossy(&current.stdout).trim().to_string();
        if normalize_remote_url(&existing) == normalize_remote_url(url) {
            return Ok(RemoteConnection::AlreadyConnected);
        }
        return Err(OpsError::InvalidInput(
            "origin is already set to a different place".to_string(),
        ));
    }
    runner.run_ok(repo, &["remote", "add", "origin", url])?;
    Ok(RemoteConnection::Added)
}

/// 初回の保存の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstSave {
    /// 初回の保存は頼まれていない
    NotRequested,
    /// 保存した
    Saved,
    /// 保存する変更がなかった
    NothingToSave,
    /// 大きいファイルがあり、利用者の決定が必要なため保存しなかった（何も変更していない）
    NeedsSizeDecision(SizeFindings),
}

/// 接続の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectOutcome {
    pub remote: RemoteConnection,
    pub first_save: FirstSave,
}

/// `origin` を設定し、頼まれていれば初回の保存まで行う。アップロードは含めない（呼び出し側が
/// 通常のアップロードを実行する。`push -u` は `upload` が行う）。
///
/// 初回の保存は通常の保存（`save`）なので、復元点の作成とサイズ検査を通る（不変条件 3）。
pub(crate) fn connect(
    runner: &GitRunner,
    repo: &Path,
    url: &str,
    first_save_memo: Option<&str>,
    limits: SizeLimits,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<ConnectOutcome, OpsError> {
    let remote = connect_remote(runner, repo, url)?;
    let first_save = match first_save_memo {
        None => FirstSave::NotRequested,
        Some(memo) => match save(
            runner,
            repo,
            memo,
            &SaveOptions {
                limits,
                ..SaveOptions::default()
            },
            now,
            meta,
        )? {
            SaveOutcome::Saved { .. } => FirstSave::Saved,
            SaveOutcome::NothingToSave => FirstSave::NothingToSave,
            SaveOutcome::NeedsSizeDecision(found) => FirstSave::NeedsSizeDecision(found),
        },
    };
    Ok(ConnectOutcome { remote, first_save })
}

/// GitHub にリポジトリを作る前の、ローカル側の検査の結果（設計書 4.6）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectPreflight {
    /// 問題が無い。リポジトリを作って接続してよい
    Ready,
    /// `origin` がすでに接続先と同じ場所を指している（既存の接続として続きから進められる）
    AlreadyPointsHere,
    /// `origin` が別の場所を指している（付け替えない）
    OriginElsewhere,
    /// `.git/config` に書き込めない（読み取り専用など）。`origin` を設定できない
    ConfigNotWritable,
    /// フォルダが見つからない、またはリポジトリではない（E11）
    FolderUnavailable(FolderState),
    /// 初回の保存の対象に大きいファイルがあり、利用者の決定が必要
    NeedsSizeDecision(SizeFindings),
}

/// `.git/config` に書き込めるか。内容は変えない（追記モードで開くだけ）。
/// `.git` がフォルダでない（作業ツリーの `.git` ファイルなど）ときは検査せず書けるものとする。
fn config_writable(repo: &Path) -> bool {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return true;
    }
    match std::fs::OpenOptions::new()
        .append(true)
        .open(git_dir.join("config"))
    {
        Ok(_) => true,
        // `git remote add` が作るため、まだ無いだけなら書けるものとする
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// GitHub にリポジトリを作る前に、ローカル側で失敗しうる点を先に確認する（読み取りのみ。
/// 作業フォルダにもインデックスにも `.git/config` にも書き込まない）。
///
/// 確認の順序: (1) フォルダが存在しリポジトリか（E11）、(2) `origin` が別の場所を指していないか
/// （同じ場所なら既存の接続）、(3) `.git/config` が書き込めるか、(4) `check_first_save` が真で、まだ
/// 保存が 1 つも無いときだけ、初回の保存の対象に保存不可・要確認の大きいファイルが無いか。
pub(crate) fn preflight_connect(
    runner: &GitRunner,
    repo: &Path,
    url: &str,
    check_first_save: bool,
    limits: SizeLimits,
) -> Result<ConnectPreflight, OpsError> {
    validate_remote_url(url)?;
    let folder = project_folder_state(repo);
    if folder.is_missing() {
        return Ok(ConnectPreflight::FolderUnavailable(folder));
    }
    let origin = origin_url(runner, repo)?;
    let points_here = match &origin {
        None => false,
        Some(existing) => {
            if normalize_remote_url(existing) != normalize_remote_url(url) {
                return Ok(ConnectPreflight::OriginElsewhere);
            }
            true
        }
    };
    // すでに同じ場所なら `origin` は書き換えないため、書き込み検査は不要
    if !points_here && !config_writable(repo) {
        return Ok(ConnectPreflight::ConfigNotWritable);
    }
    if check_first_save && commit_count(runner, repo)? == 0 {
        let findings = crate::size_check::scan(runner, repo, limits)?;
        if !findings.is_empty() {
            return Ok(ConnectPreflight::NeedsSizeDecision(findings));
        }
    }
    Ok(if points_here {
        ConnectPreflight::AlreadyPointsHere
    } else {
        ConnectPreflight::Ready
    })
}

/// `origin` の URL。設定されていなければ None（読み取りのみ）。
pub(crate) fn origin_url(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    let out = runner.run(repo, &["remote", "get-url", "origin"])?;
    if out.code != 0 {
        return Ok(None);
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!url.is_empty()).then_some(url))
}

/// 保存を 1 回でも作ったか（HEAD が指すコミットがあるか）
fn has_commits(runner: &GitRunner, repo: &Path) -> Result<bool, OpsError> {
    Ok(runner
        .run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .code
        == 0)
}

/// 現在のブランチから辿れる保存の数。まだ保存が無ければ 0。
/// 保存先は設定済みだが一度もアップロードしていない（upstream なし）ときの「アップロード待ち」に使う。
pub(crate) fn commit_count(runner: &GitRunner, repo: &Path) -> Result<u32, OpsError> {
    if !has_commits(runner, repo)? {
        return Ok(0);
    }
    let out = runner.run_ok(repo, &["rev-list", "--count", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0))
}

/// `HEAD` の保存で、まだクラウドに上がっていないもの（`@{u}..HEAD`）の完全な OID。
/// upstream が無い、または解決できない（クラウドに何も無い）ときは None。
pub(crate) fn unuploaded_commits(
    runner: &GitRunner,
    repo: &Path,
) -> Result<Option<HashSet<String>>, OpsError> {
    let upstream = runner.run(repo, &["rev-parse", "--abbrev-ref", "@{u}"])?;
    if upstream.code != 0 || !has_commits(runner, repo)? {
        return Ok(None);
    }
    let out = runner.run_ok(repo, &["rev-list", "@{u}..HEAD"])?;
    Ok(Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

/// クラウドに上がっている最新の保存（`@{u}` が指すコミット）の日時（ISO 8601）。
/// upstream が無い、または解決できないときは None。読み取りのみ。
pub(crate) fn upstream_tip_time(
    runner: &GitRunner,
    repo: &Path,
) -> Result<Option<String>, OpsError> {
    let out = runner.run(
        repo,
        &["log", "--max-count=1", "--format=%cI", "@{u}", "--"],
    )?;
    if out.code != 0 {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!text.is_empty()).then_some(text))
}

/// 保存を 1 回も作っていないとき、アップロードするものは無い（`push` は失敗してしまう）
pub(crate) fn nothing_committed(runner: &GitRunner, repo: &Path) -> Result<bool, OpsError> {
    Ok(!has_commits(runner, repo)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_with_options_whitespace_or_credentials_are_rejected() {
        for bad in [
            "",
            "-oProxyCommand=evil",
            "--upload-pack=x",
            "https://github.com/a/b .git",
            "https://github.com/a/b.git\n",
            "https://user:secret@github.com/a/b.git",
            "https://x-access-token@github.com/a/b.git",
        ] {
            assert!(validate_remote_url(bad).is_err(), "{bad:?}");
        }
        for ok in [
            "https://github.com/alice/thesis.git",
            "C:\\work\\remote.git",
            "/tmp/remote.git",
        ] {
            assert!(validate_remote_url(ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn folder_state_distinguishes_missing_file_and_non_repository() {
        let tmp = std::env::temp_dir().join(format!("core-ops-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(project_folder_state(&tmp), FolderState::Missing);
        std::fs::create_dir_all(&tmp).expect("mkdir");
        assert_eq!(project_folder_state(&tmp), FolderState::NotARepository);
        std::fs::create_dir_all(tmp.join(".git")).expect("mkdir .git");
        assert_eq!(project_folder_state(&tmp), FolderState::Available);
        let file = tmp.join("a.txt");
        std::fs::write(&file, "x").expect("write");
        assert_eq!(project_folder_state(&file), FolderState::NotADirectory);
        assert!(project_folder_state(&file).is_missing());
        std::fs::remove_dir_all(&tmp).expect("cleanup");
    }
}
