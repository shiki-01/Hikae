// 保存・取り込み・アップロード・ぶつかり解消などの高層操作 API。

mod memo;
mod models;
mod open_path;
mod operations;

pub use memo::{suggest_memo, MemoChange, MemoChangeKind, MemoLabels};
pub use models::{
    new_restore_points, Choice, ConflictFile, ConflictKind, DiffLine, DiffLineKind, FileInHistory,
    HistoryEntry, Identity, Labels, OpsError, PullOutcome, ResolveOutcome, RestoreFileChange,
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
        operations::history(self.runner(), repo, max_count)
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
    pub fn restore(&self, repo: &Path, target_commit: &str) -> Result<(), OpsError> {
        operations::restore(self.runner(), repo, target_commit, self.now())
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
