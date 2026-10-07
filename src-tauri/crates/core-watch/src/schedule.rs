// 定期的な取り込み・アップロードの実行計画（設計書 4.3 / 4.5 / 7章）。
//
// 時刻は呼び出し側が渡す単調増加の秒数（`now`）で扱う純粋なロジックで、
// 実際の実行・待機・イベント通知は app 層が担当する。
// オフラインや認証失敗は例外にせず、状態（`SyncHealth`）と再試行時刻として表す。

/// 失敗の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// ネットワークに届かない
    Offline,
    /// 認証に失敗した（トークンの失効・権限不足）
    AuthFailed,
    /// 変更のぶつかりが未解消
    Conflict,
    /// それ以外
    Other,
}

/// git のエラー出力（stderr）から失敗の種類を判定する。
pub fn classify_failure(stderr: &str) -> FailureKind {
    let s = stderr.to_ascii_lowercase();

    const AUTH: &[&str] = &[
        "authentication failed",
        "could not read username",
        "could not read password",
        "invalid username or password",
        "terminal prompts disabled",
        "permission denied (publickey)",
        "returned error: 401",
        "returned error: 403",
    ];
    if AUTH.iter().any(|p| s.contains(p)) {
        return FailureKind::AuthFailed;
    }

    const OFFLINE: &[&str] = &[
        "could not resolve host",
        "failed to connect",
        "connection timed out",
        "connection refused",
        "connection reset",
        "network is unreachable",
        "operation timed out",
        "unable to access",
        "could not read from remote repository",
        "the remote end hung up",
        "early eof",
        "could not resolve proxy",
    ];
    if OFFLINE.iter().any(|p| s.contains(p)) {
        return FailureKind::Offline;
    }

    FailureKind::Other
}

/// 実行する処理
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTask {
    /// 取り込み
    Pull,
    /// アップロード
    Push,
}

/// 処理の結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskResult {
    Ok,
    Failed(FailureKind),
}

/// プロジェクトの同期状態（ヘッダー表示・通知用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncHealth {
    /// 問題なし
    Idle,
    /// オフライン（自動で再試行する）
    Offline,
    /// 再認証が必要
    AuthRequired,
    /// 変更のぶつかりの解消待ち
    Conflicted,
    /// そのほかの失敗（自動で再試行する）
    Error,
}

/// 自動実行の設定（7章）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncPolicy {
    /// 起動時に取り込む（初期値: オン）
    pub pull_on_startup: bool,
    /// 定期的に取り込む間隔（秒）。`None` は定期取り込みなし（初期値: 15 分）
    pub pull_interval_secs: Option<u64>,
    /// 保存時に自動アップロード（初期値: オン）
    pub auto_push: bool,
}

impl Default for SyncPolicy {
    fn default() -> Self {
        SyncPolicy {
            pull_on_startup: true,
            pull_interval_secs: Some(15 * 60),
            auto_push: true,
        }
    }
}

impl SyncPolicy {
    /// プロジェクト設定の値から作る。`None` は 7章の初期値。間隔 0 は「オフ」。
    pub fn from_config(
        pull_on_startup: Option<bool>,
        pull_interval_minutes: Option<u32>,
        auto_push: Option<bool>,
    ) -> Self {
        let d = SyncPolicy::default();
        SyncPolicy {
            pull_on_startup: pull_on_startup.unwrap_or(d.pull_on_startup),
            pull_interval_secs: match pull_interval_minutes {
                None => d.pull_interval_secs,
                Some(0) => None,
                Some(m) => Some(u64::from(m) * 60),
            },
            auto_push: auto_push.unwrap_or(d.auto_push),
        }
    }
}

/// 変更のぶつかりが残っている間の再確認間隔（秒）。確認は `status` の読み取りだけで軽い。
const CONFLICT_RECHECK_SECS: u64 = 15 * 60;

/// 失敗後の再試行までの待ち時間（秒）。失敗が続くほど延ばし、上限で頭打ちにする。
pub fn retry_delay_secs(kind: FailureKind, consecutive_failures: u32) -> u64 {
    let (base, cap): (u64, u64) = match kind {
        FailureKind::Offline => (30, 10 * 60),
        // 再認証されるまで成功しないため、間隔を長めにする
        FailureKind::AuthFailed => (5 * 60, 60 * 60),
        FailureKind::Conflict => return CONFLICT_RECHECK_SECS,
        FailureKind::Other => (60, 30 * 60),
    };
    let shift = consecutive_failures.saturating_sub(1).min(16);
    base.saturating_mul(1u64 << shift).min(cap)
}

/// 1 種類の処理（取り込み or アップロード）の予定
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    next_at: Option<u64>,
    failures: u32,
    last_failure: Option<FailureKind>,
}

impl Slot {
    fn due(&self, now: u64) -> bool {
        self.next_at.is_some_and(|t| t <= now)
    }

    /// 既存の予定より早い場合のみ前倒しする
    fn schedule_no_later_than(&mut self, at: u64) {
        self.next_at = Some(self.next_at.map_or(at, |t| t.min(at)));
    }
}

/// 1 プロジェクト分の実行計画
#[derive(Debug, Clone)]
pub struct SyncPlanner {
    policy: SyncPolicy,
    pull: Slot,
    push: Slot,
}

impl SyncPlanner {
    /// 起動時に作る。「起動時に取り込む」がオンなら取り込みは直ちに実行対象になる。
    pub fn new(policy: SyncPolicy, now: u64) -> Self {
        let next_pull = if policy.pull_on_startup {
            Some(now)
        } else {
            policy.pull_interval_secs.map(|i| now.saturating_add(i))
        };
        SyncPlanner {
            policy,
            pull: Slot {
                next_at: next_pull,
                ..Slot::default()
            },
            push: Slot::default(),
        }
    }

    pub fn policy(&self) -> SyncPolicy {
        self.policy
    }

    /// 設定の変更を反映する。同じ設定なら何もしない。
    pub fn set_policy(&mut self, policy: SyncPolicy, now: u64) {
        if policy == self.policy {
            return;
        }
        self.policy = policy;
        // 失敗の再試行待ちでなければ、新しい間隔で予定を組み直す
        if self.pull.failures == 0 {
            self.pull.next_at = policy.pull_interval_secs.map(|i| now.saturating_add(i));
        }
        if !policy.auto_push && self.push.failures == 0 {
            self.push.next_at = None;
        }
    }

    /// いま実行すべき処理。取り込みを先にする（アップロードは相手が先に進んでいると取り込みが必要なため）。
    pub fn next_task(&self, now: u64) -> Option<SyncTask> {
        if self.pull.due(now) {
            Some(SyncTask::Pull)
        } else if self.push.due(now) {
            Some(SyncTask::Push)
        } else {
            None
        }
    }

    /// 次に起きるべき時刻（予定がなければ `None`）
    pub fn next_wake(&self) -> Option<u64> {
        match (self.pull.next_at, self.push.next_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// 保存が完了したときに呼ぶ。「保存時に自動アップロード」がオンならアップロードを予定する。
    pub fn request_push(&mut self, now: u64) {
        if self.policy.auto_push {
            self.push.schedule_no_later_than(now);
        }
    }

    /// 取り込みを直ちに予定する（ぶつかり解消後など）
    pub fn request_pull(&mut self, now: u64) {
        self.pull.schedule_no_later_than(now);
    }

    /// 失敗の再試行待ちを解除して直ちに再試行できるようにする（再認証後・接続回復後など）
    pub fn retry_now(&mut self, now: u64) {
        for slot in [&mut self.pull, &mut self.push] {
            if slot.last_failure.is_some() {
                slot.failures = 0;
                slot.last_failure = None;
                slot.next_at = Some(now);
            }
        }
    }

    /// 処理の結果を反映し、次回の予定を決める。
    pub fn report(&mut self, task: SyncTask, result: TaskResult, now: u64) {
        match (task, result) {
            (SyncTask::Pull, TaskResult::Ok) => {
                self.pull.failures = 0;
                self.pull.last_failure = None;
                self.pull.next_at = self
                    .policy
                    .pull_interval_secs
                    .map(|i| now.saturating_add(i));
                // 取り込み後は、マージで増えた分や過去の未送信分を送るため（4.3 手順 7）
                if self.policy.auto_push {
                    self.push.schedule_no_later_than(now);
                }
            }
            (SyncTask::Push, TaskResult::Ok) => {
                self.push.failures = 0;
                self.push.last_failure = None;
                self.push.next_at = None;
            }
            (SyncTask::Pull, TaskResult::Failed(kind)) => {
                Self::fail(&mut self.pull, kind, now);
            }
            (SyncTask::Push, TaskResult::Failed(kind)) => {
                Self::fail(&mut self.push, kind, now);
            }
        }
    }

    fn fail(slot: &mut Slot, kind: FailureKind, now: u64) {
        slot.failures = slot.failures.saturating_add(1);
        slot.last_failure = Some(kind);
        slot.next_at = Some(now.saturating_add(retry_delay_secs(kind, slot.failures)));
    }

    /// 現在の同期状態。取り込みとアップロードの失敗のうち、ユーザーの対応が必要なものを優先する。
    pub fn health(&self) -> SyncHealth {
        let kinds = [self.pull.last_failure, self.push.last_failure];
        let has = |k: FailureKind| kinds.contains(&Some(k));
        if has(FailureKind::AuthFailed) {
            SyncHealth::AuthRequired
        } else if has(FailureKind::Conflict) {
            SyncHealth::Conflicted
        } else if has(FailureKind::Offline) {
            SyncHealth::Offline
        } else if has(FailureKind::Other) {
            SyncHealth::Error
        } else {
            SyncHealth::Idle
        }
    }

    /// 失敗後の再試行予定時刻（失敗中でなければ `None`）
    pub fn retry_at(&self) -> Option<u64> {
        [&self.pull, &self.push]
            .into_iter()
            .filter(|s| s.last_failure.is_some())
            .filter_map(|s| s.next_at)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60;

    fn failed(kind: FailureKind) -> TaskResult {
        TaskResult::Failed(kind)
    }

    #[test]
    fn defaults_follow_chapter_7() {
        let p = SyncPolicy::default();
        assert!(p.pull_on_startup && p.auto_push);
        assert_eq!(p.pull_interval_secs, Some(15 * MIN));
        assert_eq!(SyncPolicy::from_config(None, None, None), p);
        assert_eq!(
            SyncPolicy::from_config(None, Some(0), None).pull_interval_secs,
            None
        );
        assert_eq!(
            SyncPolicy::from_config(Some(false), Some(5), Some(false)),
            SyncPolicy {
                pull_on_startup: false,
                pull_interval_secs: Some(5 * MIN),
                auto_push: false
            }
        );
    }

    #[test]
    fn startup_pull_is_due_immediately_then_follows_interval() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 100);
        assert_eq!(p.next_task(100), Some(SyncTask::Pull));
        p.report(SyncTask::Pull, TaskResult::Ok, 100);
        // 取り込み成功後はアップロード確認が予定される
        assert_eq!(p.next_task(100), Some(SyncTask::Push));
        p.report(SyncTask::Push, TaskResult::Ok, 101);
        assert_eq!(p.next_task(101), None);
        // 次の取り込みは 15 分後
        assert_eq!(p.next_wake(), Some(100 + 15 * MIN));
        assert_eq!(p.next_task(100 + 15 * MIN - 1), None);
        assert_eq!(p.next_task(100 + 15 * MIN), Some(SyncTask::Pull));
    }

    #[test]
    fn startup_pull_can_be_disabled() {
        let policy = SyncPolicy {
            pull_on_startup: false,
            ..SyncPolicy::default()
        };
        let p = SyncPlanner::new(policy, 0);
        assert_eq!(p.next_task(0), None);
        assert_eq!(p.next_task(15 * MIN), Some(SyncTask::Pull));

        let off = SyncPolicy {
            pull_on_startup: false,
            pull_interval_secs: None,
            auto_push: true,
        };
        let p = SyncPlanner::new(off, 0);
        assert_eq!(p.next_wake(), None);
        assert_eq!(p.next_task(u64::MAX), None);
    }

    #[test]
    fn save_requests_push_only_when_auto_push_is_on() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.report(SyncTask::Pull, TaskResult::Ok, 0);
        p.report(SyncTask::Push, TaskResult::Ok, 0);
        p.request_push(50);
        assert_eq!(p.next_task(50), Some(SyncTask::Push));

        let off = SyncPolicy {
            auto_push: false,
            ..SyncPolicy::default()
        };
        let mut p = SyncPlanner::new(off, 0);
        p.report(SyncTask::Pull, TaskResult::Ok, 0);
        p.request_push(50);
        // 取り込み成功後も自動アップロードはしない
        assert_eq!(p.next_task(50), None);
    }

    #[test]
    fn pull_runs_before_push() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.request_push(0);
        assert_eq!(p.next_task(0), Some(SyncTask::Pull));
    }

    #[test]
    fn offline_failures_back_off_and_cap() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        let mut now = 0;
        let mut delays = Vec::new();
        for _ in 0..8 {
            p.report(SyncTask::Pull, failed(FailureKind::Offline), now);
            let at = p.retry_at().unwrap_or(0);
            delays.push(at - now);
            assert_eq!(p.health(), SyncHealth::Offline);
            assert_eq!(p.next_task(at - 1), None);
            now = at;
        }
        assert_eq!(delays, vec![30, 60, 120, 240, 480, 600, 600, 600]);
    }

    #[test]
    fn recovery_resets_backoff_and_health() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.report(SyncTask::Pull, failed(FailureKind::Offline), 0);
        p.report(SyncTask::Pull, failed(FailureKind::Offline), 30);
        p.report(SyncTask::Pull, TaskResult::Ok, 90);
        assert_eq!(p.health(), SyncHealth::Idle);
        assert_eq!(p.retry_at(), None);
        p.report(SyncTask::Push, TaskResult::Ok, 90);
        // 失敗の履歴が消え、次の失敗は最短の待ちから始まる
        p.report(SyncTask::Pull, failed(FailureKind::Offline), 100);
        assert_eq!(p.retry_at(), Some(130));
    }

    #[test]
    fn auth_failure_waits_longer_and_takes_priority_in_health() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.report(SyncTask::Push, failed(FailureKind::AuthFailed), 0);
        p.report(SyncTask::Pull, failed(FailureKind::Offline), 0);
        assert_eq!(p.health(), SyncHealth::AuthRequired);
        assert_eq!(retry_delay_secs(FailureKind::AuthFailed, 1), 5 * MIN);
        assert_eq!(retry_delay_secs(FailureKind::AuthFailed, 30), 60 * MIN);
        // 再認証後は直ちに再試行できる
        p.retry_now(500);
        assert_eq!(p.health(), SyncHealth::Idle);
        assert_eq!(p.next_task(500), Some(SyncTask::Pull));
    }

    #[test]
    fn conflict_is_rechecked_at_fixed_interval_and_pull_request_overrides() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.report(SyncTask::Pull, failed(FailureKind::Conflict), 0);
        p.report(SyncTask::Pull, failed(FailureKind::Conflict), 15 * MIN);
        assert_eq!(p.health(), SyncHealth::Conflicted);
        assert_eq!(p.retry_at(), Some(30 * MIN));
        // 解消後は直ちに取り込みを予定できる
        p.request_pull(20 * MIN);
        assert_eq!(p.next_task(20 * MIN), Some(SyncTask::Pull));
    }

    #[test]
    fn retry_delay_never_overflows() {
        assert_eq!(retry_delay_secs(FailureKind::Other, u32::MAX), 30 * MIN);
        assert_eq!(retry_delay_secs(FailureKind::Offline, 0), 30);
    }

    #[test]
    fn policy_change_reschedules_only_when_not_backing_off() {
        let mut p = SyncPlanner::new(SyncPolicy::default(), 0);
        p.report(SyncTask::Pull, TaskResult::Ok, 0);
        let faster = SyncPolicy {
            pull_interval_secs: Some(5 * MIN),
            ..SyncPolicy::default()
        };
        p.set_policy(faster, 100);
        assert!(p.next_wake().is_some_and(|t| t <= 100 + 5 * MIN));

        p.report(SyncTask::Pull, failed(FailureKind::Offline), 1000);
        let off = SyncPolicy {
            pull_interval_secs: None,
            ..faster
        };
        p.set_policy(off, 1001);
        // 再試行待ちは設定変更で消えない
        assert_eq!(p.retry_at(), Some(1030));
    }

    #[test]
    fn classify_git_errors() {
        use FailureKind::*;
        let c = classify_failure;
        assert_eq!(
            c("fatal: unable to access 'https://github.com/o/r.git/': Could not resolve host: github.com"),
            Offline
        );
        assert_eq!(
            c("fatal: unable to access 'https://github.com/o/r.git/': Failed to connect to github.com port 443"),
            Offline
        );
        assert_eq!(
            c("remote: Invalid username or password.\nfatal: Authentication failed for 'https://github.com/o/r.git/'"),
            AuthFailed
        );
        // 403 は「unable to access」を含んでも認証失敗を優先する
        assert_eq!(
            c("fatal: unable to access 'https://github.com/o/r.git/': The requested URL returned error: 403"),
            AuthFailed
        );
        assert_eq!(
            c("fatal: could not read Username for 'https://github.com': terminal prompts disabled"),
            AuthFailed
        );
        assert_eq!(c("error: some other failure"), Other);
        assert_eq!(c(""), Other);
    }
}
