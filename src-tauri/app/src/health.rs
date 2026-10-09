// 健全性の確認（設計書 5章 E09・E10・E14・E17）。
//
// - プロジェクトごとの状態（リポジトリの破損、保存の容量）を覚えておく。検査は起動時と日次の保守、
//   保存の後、状態の取得に失敗したときに行い、`project_status` はここに覚えた結果を読むだけにする
//   （状態の取得のたびに検査の git を走らせない）
// - 状態を変える操作の前に、他の操作が `index.lock` を持っていないか確かめて待つ（E14）
// - アプリ起動時の git の確認（E17）
//
// 判断の部分は `core-ops` の純関数（`assess_index_lock`、`check_repo_health`、`probe_git` など）にあり、
// ここは OS とのつなぎ（プロセスの一覧、待ち時間、保持する状態）だけを持つ。
// **ロックファイルは削除しない。** git は `core-git` の `GitRunner` を通してだけ呼ぶ。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use core_ops::{Ops, OpsError, RepoHealth, WaitPlan};
use core_store::Project;

/// プロジェクトごとの状態の記憶
#[derive(Default)]
pub struct ProjectHealth {
    broken: Mutex<HashSet<String>>,
    large: Mutex<HashSet<String>>,
}

/// 検査で状態が変わったか（変わったときだけ画面へ知らせる）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HealthChange {
    /// 新しく破損が疑われるようになった
    pub became_broken: bool,
    /// 新しく容量が大きくなった
    pub became_large: bool,
    /// 何らかの状態が変わった（直ったものを含む）
    pub changed: bool,
}

fn set_member(set: &Mutex<HashSet<String>>, id: &str, on: bool) -> bool {
    let Ok(mut guard) = set.lock() else {
        return false;
    };
    if on {
        guard.insert(id.to_string())
    } else {
        guard.remove(id)
    }
}

fn has_member(set: &Mutex<HashSet<String>>, id: &str) -> bool {
    set.lock().map(|g| g.contains(id)).unwrap_or(false)
}

impl ProjectHealth {
    pub(crate) fn is_broken(&self, id: &str) -> bool {
        has_member(&self.broken, id)
    }

    pub(crate) fn is_large(&self, id: &str) -> bool {
        has_member(&self.large, id)
    }

    /// 変わったときだけ true
    pub(crate) fn set_broken(&self, id: &str, on: bool) -> bool {
        set_member(&self.broken, id, on)
    }

    pub(crate) fn set_large(&self, id: &str, on: bool) -> bool {
        set_member(&self.large, id, on)
    }

    /// 登録を外した・付け替えたプロジェクトの記憶を捨てる
    pub(crate) fn forget(&self, id: &str) {
        set_member(&self.broken, id, false);
        set_member(&self.large, id, false);
    }

    /// 検査して状態を更新する（読み取りのみ）。git が起動できないなど、検査自体ができなかったときは
    /// 何も変えない。壊れていると分かったプロジェクトは、容量の検査をしない。
    pub(crate) fn refresh(&self, ops: &Ops, project: &Project) -> HealthChange {
        let mut change = HealthChange::default();
        if core_ops::project_folder_state(&project.path).is_missing() {
            return change;
        }
        let expect_commits = project.initial_commit.is_some();
        match ops.check_repo_health(&project.path, expect_commits) {
            Ok(RepoHealth::Broken(_)) => {
                if self.set_broken(&project.id, true) {
                    change.became_broken = true;
                    change.changed = true;
                }
                if self.set_large(&project.id, false) {
                    change.changed = true;
                }
            }
            Ok(RepoHealth::Healthy) => {
                if self.set_broken(&project.id, false) {
                    change.changed = true;
                }
                let sub = self.refresh_size(ops, project);
                change.became_large = sub.became_large;
                change.changed |= sub.changed;
            }
            Err(_) => {}
        }
        change
    }

    /// 保存の容量だけを調べて更新する（保存の後など）。調べられなかったときは何も変えない。
    pub(crate) fn refresh_size(&self, ops: &Ops, project: &Project) -> HealthChange {
        let mut change = HealthChange::default();
        if let Ok(size) = ops.repo_size(&project.path) {
            if self.set_large(&project.id, size.exceeds_warning()) {
                change.changed = true;
                change.became_large = size.exceeds_warning();
            }
        }
        change
    }
}

// ---- E14: index.lock ----

/// 状態を変える操作の前に、`index.lock` の解放を待つ必要があるか。
/// アップロードは、取り込みを伴わない限りインデックスに触れない（拒否されたときの取り込みで
/// ロックに当たれば、git のエラーから同じ `IndexLocked` に分類される）
pub(crate) fn touches_index(operation: &str) -> bool {
    operation != "push"
}

/// 他の操作が `index.lock` を持っていれば、解放を待つ。`wait` が偽なら待たずに判定だけ行う
/// （スケジューラの自動実行で、画面の裏の直列キューを長く止めないため）。
/// ロックが古く git のプロセスも動いていなければ、待たずに `IndexLocked { stale: true }` を返す。
/// ロックファイルは削除しない。
pub(crate) fn wait_for_index(repo: &Path, wait: bool) -> Result<(), OpsError> {
    let plan = if wait {
        core_ops::DEFAULT_WAIT
    } else {
        WaitPlan {
            max_polls: 0,
            interval: Duration::ZERO,
        }
    };
    core_ops::ensure_index_free(
        repo,
        plan,
        &SystemTime::now,
        &git_process_running,
        &mut std::thread::sleep,
    )
}

/// git のプロセスが動いているか。調べられないときは「動いている」とする（古いロックと誤判定して
/// 動作中の操作を壊さないため）。git の実行ではなく、OS のプロセス一覧の確認。
pub(crate) fn git_process_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq git.exe", "/NH", "/FO", "CSV"])
            .creation_flags(0x0800_0000)
            .output();
        match output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .to_ascii_lowercase()
                .contains("git.exe"),
            _ => true,
        }
    }
    #[cfg(not(windows))]
    {
        match std::process::Command::new("pgrep")
            .args(["-x", "git"])
            .output()
        {
            // 0: 見つかった、1: 見つからなかった。それ以外（pgrep が無いなど）は調べられない
            Ok(out) => match out.status.code() {
                Some(0) => true,
                Some(1) => false,
                _ => true,
            },
            Err(_) => true,
        }
    }
}

// ---- E17: git の確認 ----

/// アプリが使う git（同梱、無ければ PATH 上）が実行できるかを確かめる。`GitRunner` 経由で、
/// リポジトリではない一時フォルダに対して読み取りのコマンドを 1 回実行するだけ。
pub(crate) fn probe_app_git() -> (core_ops::GitProbe, core_git::GitSource) {
    let runner = crate::git_runner();
    let probe = core_ops::probe_git(&runner, &std::env::temp_dir());
    (probe, runner.source())
}
