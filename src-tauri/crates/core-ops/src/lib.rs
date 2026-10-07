// 保存・取り込み・アップロード・ぶつかり解消などの高層操作 API。

mod models;
mod operations;

pub use models::{
    Choice, ConflictFile, ConflictKind, Identity, OpsError, PullOutcome, ResolveOutcome,
    SaveOutcome, SyncState, UploadOutcome,
};

use core_git::GitRunner;
use std::path::Path;
use std::sync::Arc;
use time::OffsetDateTime;

/// 高層操作のランナー。git および safety API を使用する。
pub struct Ops {
    runner: Arc<GitRunner>,
    clock: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
}

impl Ops {
    /// GitRunner を指定して新規作成。時刻は OffsetDateTime::now_utc を使用。
    pub fn new(runner: GitRunner) -> Self {
        let r = Arc::new(runner);
        Ops {
            runner: r,
            clock: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// テスト用に時刻ソースを変更できるビルダーメソッド。
    pub fn with_clock(mut self, f: impl Fn() -> OffsetDateTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(f);
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
        operations::pull(self.runner(), repo, self.now())
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
        )
    }

    /// merge をキャンセル。復元点は残る。
    pub fn abort_merge(&self, repo: &Path) -> Result<(), OpsError> {
        operations::abort_merge(self.runner(), repo, self.now())
    }

    /// upstream に push する。拒否されたら pull→再試行。
    pub fn upload(&self, repo: &Path) -> Result<UploadOutcome, OpsError> {
        operations::upload(self.runner(), repo, self.now())
    }

    /// 同期状態を返す。ahead / behind / has_upstream / dirty。
    pub fn sync_state(&self, repo: &Path) -> Result<SyncState, OpsError> {
        operations::sync_state(self.runner(), repo)
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
