// 保存・取り込み・アップロード・ぶつかり解消などの高層操作 API。

mod add_files;
mod clone_dest;
mod file_in_use;
mod history;
mod identity;
mod memo;
mod models;
mod open_path;
mod operations;
mod relocate;
mod restore_file;

pub use add_files::{LARGE_FILE_LIMIT_BYTES, LARGE_FILE_WARN_BYTES};
pub use clone_dest::{check_clone_destination, CloneDestinationError};
pub use identity::{resolve_identity, FALLBACK_EMAIL, FALLBACK_NAME};
pub use memo::{suggest_memo, MemoChange, MemoChangeKind, MemoLabels};
pub use models::{
    new_restore_points, AddFilesOutcome, AddRejectReason, AddedFile, Choice, ConflictFile,
    ConflictKind, DiffLine, DiffLineKind, FileInHistory, HistoryEntry, HistoryKind, Identity,
    Labels, OpsError, PointChange, PointChangeKind, PullOutcome, RejectedFile, RelocateCheck,
    ResolveOutcome, RestoreFileChange, RestoreFileKind, RestoreFileOutcome, RestoreFilePreview,
    RestorePointInfo, RestorePreview, SaveOutcome, SyncState, UploadOutcome,
};
pub use open_path::{resolve_in_project, OpenPathError};

use core_git::GitRunner;
use std::path::Path;
use std::sync::Arc;
use time::OffsetDateTime;

/// 高層操作のランナー。git および safety API を使用する。
pub struct Ops {
    runner: Arc<GitRunner>,
    clock: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
    labels: Labels,
}

impl Ops {
    /// GitRunner を指定して新規作成。時刻は OffsetDateTime::now_utc を使用。
    pub fn new(runner: GitRunner) -> Self {
        let r = Arc::new(runner);
        Ops {
            runner: r,
            clock: Arc::new(OffsetDateTime::now_utc),
            labels: Labels::default(),
        }
    }

    /// テスト用に時刻ソースを変更できるビルダーメソッド。
    pub fn with_clock(mut self, f: impl Fn() -> OffsetDateTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(f);
        self
    }

    /// 履歴メモ・別名コピーに使う文言を差し替える。
    pub fn with_labels(mut self, labels: Labels) -> Self {
        self.labels = labels;
        self
    }

    fn runner(&self) -> &GitRunner {
        &self.runner
    }

    fn now(&self) -> OffsetDateTime {
        (self.clock)()
    }

    /// リポジトリの初期化。remote_url があれば remote add も行う。
    /// identity に GitHub のユーザー名と noreply アドレスを渡す。
    pub fn init_project(
        &self,
        dir: &Path,
        remote_url: Option<&str>,
        identity: &Identity,
    ) -> Result<(), OpsError> {
        operations::init_project(self.runner(), dir, remote_url, identity, self.now())
    }

    /// リポジトリ単位の署名（user.name / user.email）を更新する。
    /// 作業フォルダとインデックスは変更しない（.git/config のみ）。グローバル設定へは書かない。
    pub fn apply_identity(&self, repo: &Path, identity: &Identity) -> Result<(), OpsError> {
        operations::apply_identity(self.runner(), repo, identity)
    }

    /// URL から clone し、identity を設定。
    pub fn clone_project(
        &self,
        url: &str,
        dest: &Path,
        identity: &Identity,
    ) -> Result<(), OpsError> {
        operations::clone_project(self.runner(), url, dest, identity, self.now())
    }

    /// 変更をコミット。未保存変更がなければ NothingToSave を返す。
    /// 変更があれば復元点を作成し、add -A, commit を実行する。
    pub fn save(&self, repo: &Path, memo: &str) -> Result<SaveOutcome, OpsError> {
        operations::save(self.runner(), repo, memo, self.now())
    }

    /// upstream から取り込む。未保存変更があれば自動保存してから取り込む。
    pub fn pull(&self, repo: &Path) -> Result<PullOutcome, OpsError> {
        operations::pull(self.runner(), repo, self.now(), &self.labels)
    }

    /// 現在の競合ファイル一覧を返す。
    pub fn conflicts(&self, repo: &Path) -> Result<Vec<ConflictFile>, OpsError> {
        operations::conflicts(self.runner(), repo)
    }

    /// 競合を解消。全競合ファイルについて choices を指定。
    pub fn resolve(
        &self,
        repo: &Path,
        choices: &[(String, Choice)],
        keep_other_copy: bool,
        message: &str,
    ) -> Result<ResolveOutcome, OpsError> {
        operations::resolve(
            self.runner(),
            repo,
            choices,
            keep_other_copy,
            message,
            self.now(),
            &self.labels,
        )
    }

    /// merge をキャンセル。復元点は残る。
    pub fn abort_merge(&self, repo: &Path) -> Result<(), OpsError> {
        operations::abort_merge(self.runner(), repo, self.now())
    }

    /// upstream に push する。拒否されたら pull→再試行。
    pub fn upload(&self, repo: &Path) -> Result<UploadOutcome, OpsError> {
        operations::upload(self.runner(), repo, self.now(), &self.labels)
    }

    /// 同期状態を返す。ahead / behind / has_upstream / dirty。
    pub fn sync_state(&self, repo: &Path) -> Result<SyncState, OpsError> {
        operations::sync_state(self.runner(), repo)
    }

    /// 履歴一覧を取得。最新順。refs/hikae/ の自動保存は区別される。
    pub fn history(&self, repo: &Path, max_count: usize) -> Result<Vec<HistoryEntry>, OpsError> {
        history::history(self.runner(), repo, max_count)
    }

    /// 特定時点のファイル一覧を取得。
    pub fn list_files_at(&self, repo: &Path, commit: &str) -> Result<Vec<FileInHistory>, OpsError> {
        operations::list_files_at(self.runner(), repo, commit)
    }

    /// 未保存の変更からルールベースで保存メモの案を作る（設計書 10.3）。変更がなければ空文字列。
    pub fn suggest_memo(&self, repo: &Path, labels: &MemoLabels) -> Result<String, OpsError> {
        operations::suggest_memo_for(self.runner(), repo, labels)
    }

    /// 復元点（隠し ref）の一覧。操作の前後で比べ、操作が作った復元点の特定に使う。
    pub fn restore_point_refs(&self, repo: &Path) -> Result<Vec<String>, OpsError> {
        operations::restore_point_refs(self.runner(), repo)
    }

    /// 2つの時点の差分を取得（行単位）。
    pub fn diff_with(
        &self,
        repo: &Path,
        from: &str,
        to: &str,
        path: Option<&str>,
    ) -> Result<Vec<DiffLine>, OpsError> {
        operations::diff_with(self.runner(), repo, from, to, path)
    }

    /// 元に戻す操作のプレビュー（影響ファイル一覧）。
    pub fn restore_preview(
        &self,
        repo: &Path,
        target_commit: &str,
    ) -> Result<RestorePreview, OpsError> {
        operations::restore_preview(self.runner(), repo, target_commit)
    }

    /// 指定の時点へ復元。復元前に復元点を作成し、指定時点のファイル状態に復元。
    /// 戻り値は取り消し用の復元点（`refs/hikae/` 配下の ref 名）。`undo_restore` にそのまま渡せる。
    pub fn restore(&self, repo: &Path, target_commit: &str) -> Result<Option<String>, OpsError> {
        operations::restore(self.runner(), repo, target_commit, self.now())
    }

    /// 1 ファイルだけを戻した場合の影響（読み取りのみ）。
    pub fn restore_file_preview(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
    ) -> Result<RestoreFilePreview, OpsError> {
        restore_file::restore_file_preview(self.runner(), repo, commit, path)
    }

    /// 指定時点の 1 ファイルだけを作業フォルダに戻す。他のファイルは変更しない。
    /// 戻す前に復元点を作る。その時点に無いファイルは何も変更せず `NotInThatPoint` を返す。
    pub fn restore_file(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
    ) -> Result<RestoreFileOutcome, OpsError> {
        restore_file::restore_file(self.runner(), repo, commit, path, self.now())
    }

    /// 復元点（`refs/hikae/` 配下の ref 名、または完全な OID）の内容に戻す。
    /// 戻す前に、いまの状態の復元点を作る。
    pub fn undo_restore(&self, repo: &Path, restore_point: &str) -> Result<(), OpsError> {
        restore_file::undo_restore(self.runner(), repo, restore_point, self.now())
    }

    /// その保存で変更されたファイルの一覧（変更の種類つき）。
    pub fn list_point_changes(
        &self,
        repo: &Path,
        commit: &str,
    ) -> Result<Vec<PointChange>, OpsError> {
        restore_file::list_point_changes(self.runner(), repo, commit)
    }

    /// 外部のファイルをプロジェクト配下へコピーする（上書きしない・保存はしない）。
    /// `dest_subdir` はプロジェクトからの相対パス。空ならプロジェクト直下。
    pub fn add_files(
        &self,
        repo: &Path,
        sources: &[std::path::PathBuf],
        dest_subdir: &str,
    ) -> Result<AddFilesOutcome, OpsError> {
        add_files::add_files(self.runner(), repo, sources, dest_subdir, self.now())
    }

    /// 付け替え先のフォルダが登録済みプロジェクトと同じリポジトリか確認する（ファイルは変更しない）。
    pub fn check_relocation(
        &self,
        new_path: &Path,
        expected_remote: Option<&str>,
    ) -> Result<RelocateCheck, OpsError> {
        self.check_relocation_with(new_path, expected_remote, None)
    }

    /// `check_relocation` に、保存先 URL が無い場合の照合用として初期の保存（最初の commit）の OID を加えた版。
    pub fn check_relocation_with(
        &self,
        new_path: &Path,
        expected_remote: Option<&str>,
        expected_initial_commit: Option<&str>,
    ) -> Result<RelocateCheck, OpsError> {
        relocate::check_relocation(
            self.runner(),
            new_path,
            expected_remote,
            expected_initial_commit,
        )
    }

    /// 最初の保存（親を持たない commit）の OID。まだ保存が無ければ None。
    /// プロジェクト登録時に記録し、保存先 URL の無いプロジェクトの付け替え照合に使う。
    pub fn initial_commit(&self, repo: &Path) -> Result<Option<String>, OpsError> {
        relocate::initial_commit(self.runner(), repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_new() {
        let runner = GitRunner::from_path_env();
        let ops = Ops::new(runner);
        let now = ops.now();
        assert!(now <= OffsetDateTime::now_utc());
    }

    #[test]
    fn test_ops_with_clock() {
        let runner = GitRunner::from_path_env();
        let fixed_time = OffsetDateTime::now_utc();
        let ops = Ops::new(runner).with_clock(move || fixed_time);
        let t1 = ops.now();
        let t2 = ops.now();
        assert_eq!(t1, t2);
        assert_eq!(t1, fixed_time);
    }
}
