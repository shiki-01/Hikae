// 自動保存（スナップショット）をいつ作るかの判断（設計書 6.2、7章「自動保存」）。
//
// 最後のファイル変更から `delay_secs` 秒、新しい変更が無かった（静止した）ときに作る。
// 時刻は呼び出し側が渡す単調増加の秒数（`now`）で扱う純粋なロジックで、実際の作成・待機・
// 通知は app 層が担当する。失敗したときは待ち時間を延ばして再試行する。

/// 失敗後の再試行までの最初の待ち時間（秒）
const RETRY_BASE_SECS: u64 = 60;
/// 失敗後の再試行の待ち時間の上限（秒）
const RETRY_CAP_SECS: u64 = 30 * 60;
/// 実行を見送った（ぶつかりの解消中など）ときに、もう一度判断するまでの待ち時間（秒）
pub const DEFER_SECS: u64 = 60;

/// 自動保存の設定（7章）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPolicy {
    /// ファイル監視による自動保存（初期値: オン）
    pub enabled: bool,
    /// 最後の変更からこの秒数だけ静止してから作る（初期値: 120 秒）
    pub delay_secs: u64,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        SnapshotPolicy {
            enabled: true,
            delay_secs: 120,
        }
    }
}

impl SnapshotPolicy {
    /// 設定の値から作る
    pub fn from_settings(enabled: bool, delay_secs: u32) -> Self {
        SnapshotPolicy {
            enabled,
            delay_secs: u64::from(delay_secs),
        }
    }
}

/// 自動保存を試みた結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotAttempt {
    /// 作った、または作る必要が無かった（内容が直前と同じ・変更なし）
    Completed,
    /// 今は作れない（ぶつかりの解消中など）。少し待ってからもう一度判断する
    Deferred,
    /// 失敗した。待ち時間を延ばして再試行する
    Failed,
}

/// 失敗後の再試行までの待ち時間（秒）。連続で失敗するほど延ばし、上限で頭打ちにする。
pub fn retry_delay_secs(consecutive_failures: u32) -> u64 {
    let shift = consecutive_failures.saturating_sub(1).min(16);
    RETRY_BASE_SECS
        .saturating_mul(1u64 << shift)
        .min(RETRY_CAP_SECS)
}

/// 1 プロジェクト分の、自動保存の実行計画
#[derive(Debug, Clone)]
pub struct SnapshotPlanner {
    policy: SnapshotPolicy,
    /// 最後にファイルが変わった時刻。未保存の変更の記録が無ければ None
    last_change: Option<u64>,
    /// 実行中の試行が対象にした「最後の変更」の時刻
    in_flight: Option<u64>,
    /// これより前には再試行しない時刻
    not_before: Option<u64>,
    failures: u32,
}

impl SnapshotPlanner {
    pub fn new(policy: SnapshotPolicy) -> Self {
        SnapshotPlanner {
            policy,
            last_change: None,
            in_flight: None,
            not_before: None,
            failures: 0,
        }
    }

    pub fn policy(&self) -> SnapshotPolicy {
        self.policy
    }

    /// 設定の変更を反映する。オフにしたら待ち中の予定を捨てる（オフの間の変更は覚えない）。
    pub fn set_policy(&mut self, policy: SnapshotPolicy) {
        if !policy.enabled {
            self.last_change = None;
            self.not_before = None;
            self.failures = 0;
        }
        self.policy = policy;
    }

    /// ファイルが変わった。待ち中の予定が無かった状態から待ち中になったときだけ true
    /// （その場合は、実行する時刻に起きられるよう呼び出し側が待機を組み直す）。
    pub fn note_change(&mut self, now: u64) -> bool {
        if !self.policy.enabled {
            return false;
        }
        let was_pending = self.last_change.is_some();
        self.last_change = Some(now);
        !was_pending
    }

    /// 自動保存を作る予定が待っているか
    pub fn is_pending(&self) -> bool {
        self.policy.enabled && self.last_change.is_some()
    }

    /// いま試みる時期か（最後の変更から `delay_secs` 秒が過ぎ、再試行待ちでもない）
    pub fn is_due(&self, now: u64) -> bool {
        self.next_at().is_some_and(|at| at <= now)
    }

    /// 試みる予定の時刻。待ち中の予定が無ければ None
    pub fn next_at(&self) -> Option<u64> {
        if !self.is_pending() {
            return None;
        }
        let quiet_at = self.last_change?.saturating_add(self.policy.delay_secs);
        Some(quiet_at.max(self.not_before.unwrap_or(0)))
    }

    /// 試みを始める。対象にした「最後の変更」を覚え、実行中に新しい変更が来たかを後で区別する。
    /// 試みる時期でなければ false。
    pub fn begin(&mut self, now: u64) -> bool {
        if !self.is_due(now) {
            return false;
        }
        self.in_flight = self.last_change;
        true
    }

    /// 試みの結果を反映する。
    pub fn finish(&mut self, result: SnapshotAttempt, now: u64) {
        let handled = self.in_flight.take();
        match result {
            SnapshotAttempt::Completed => {
                self.failures = 0;
                self.not_before = None;
                // 実行中に新しい変更が来ていたら、その分は次の静止を待って改めて作る
                if self.last_change == handled {
                    self.last_change = None;
                }
            }
            SnapshotAttempt::Deferred => {
                self.not_before = Some(now.saturating_add(DEFER_SECS));
            }
            SnapshotAttempt::Failed => {
                self.failures = self.failures.saturating_add(1);
                self.not_before = Some(now.saturating_add(retry_delay_secs(self.failures)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner(delay: u64) -> SnapshotPlanner {
        SnapshotPlanner::new(SnapshotPolicy {
            enabled: true,
            delay_secs: delay,
        })
    }

    #[test]
    fn defaults_follow_chapter_7() {
        let p = SnapshotPolicy::default();
        assert!(p.enabled);
        assert_eq!(p.delay_secs, 120);
        assert_eq!(
            SnapshotPolicy::from_settings(false, 30),
            SnapshotPolicy {
                enabled: false,
                delay_secs: 30
            }
        );
    }

    #[test]
    fn nothing_is_due_without_a_change() {
        let p = planner(120);
        assert!(!p.is_pending());
        assert_eq!(p.next_at(), None);
        assert!(!p.is_due(u64::MAX));
    }

    #[test]
    fn becomes_due_after_the_quiet_period_from_the_last_change() {
        let mut p = planner(120);
        assert!(p.note_change(1_000));
        assert_eq!(p.next_at(), Some(1_120));
        assert!(!p.is_due(1_119));
        assert!(p.is_due(1_120));

        // 変更が続く間は後ろへずれる（最後の変更から数える）
        assert!(!p.note_change(1_100));
        assert_eq!(p.next_at(), Some(1_220));
        assert!(!p.is_due(1_219));
        assert!(p.is_due(1_220));
    }

    #[test]
    fn begin_requires_the_due_time_and_completed_clears_the_pending_change() {
        let mut p = planner(30);
        p.note_change(0);
        assert!(!p.begin(29));
        assert!(p.begin(30));
        p.finish(SnapshotAttempt::Completed, 31);
        assert!(!p.is_pending());
        assert_eq!(p.next_at(), None);
    }

    #[test]
    fn a_change_during_the_attempt_schedules_another_one() {
        let mut p = planner(30);
        p.note_change(0);
        assert!(p.begin(30));
        // 実行中に新しい変更
        assert!(!p.note_change(35));
        p.finish(SnapshotAttempt::Completed, 40);
        assert!(p.is_pending());
        assert_eq!(p.next_at(), Some(65));
    }

    #[test]
    fn failures_back_off_and_cap_then_recover() {
        let mut p = planner(10);
        p.note_change(0);
        let mut now = 10;
        let mut waits = Vec::new();
        for _ in 0..8 {
            assert!(p.begin(now));
            p.finish(SnapshotAttempt::Failed, now);
            let next = p.next_at().unwrap_or(0);
            waits.push(next - now);
            assert!(!p.is_due(next - 1));
            now = next;
        }
        assert_eq!(waits, vec![60, 120, 240, 480, 960, 1_800, 1_800, 1_800]);
        // 成功すれば待ち時間は最初に戻る
        assert!(p.begin(now));
        p.finish(SnapshotAttempt::Completed, now);
        p.note_change(now + 1);
        assert!(p.begin(now + 11));
        p.finish(SnapshotAttempt::Failed, now + 11);
        assert_eq!(p.next_at(), Some(now + 11 + 60));
    }

    #[test]
    fn deferred_waits_a_fixed_time_without_counting_as_failure() {
        let mut p = planner(10);
        p.note_change(0);
        assert!(p.begin(10));
        p.finish(SnapshotAttempt::Deferred, 10);
        assert_eq!(p.next_at(), Some(10 + DEFER_SECS));
        assert!(p.begin(10 + DEFER_SECS));
        p.finish(SnapshotAttempt::Deferred, 10 + DEFER_SECS);
        assert_eq!(p.next_at(), Some(10 + 2 * DEFER_SECS));
    }

    #[test]
    fn disabling_drops_the_pending_change_and_ignores_changes_while_off() {
        let mut p = planner(30);
        p.note_change(0);
        p.set_policy(SnapshotPolicy {
            enabled: false,
            delay_secs: 30,
        });
        assert!(!p.is_pending());
        assert!(!p.note_change(5));
        assert!(!p.is_due(u64::MAX));
        // オンに戻しても、オフの間の変更は覚えていない
        p.set_policy(SnapshotPolicy {
            enabled: true,
            delay_secs: 30,
        });
        assert!(!p.is_pending());
        assert!(p.note_change(100));
        assert_eq!(p.next_at(), Some(130));
    }

    #[test]
    fn changing_the_delay_applies_to_the_pending_change() {
        let mut p = planner(600);
        p.note_change(0);
        p.set_policy(SnapshotPolicy {
            enabled: true,
            delay_secs: 30,
        });
        assert_eq!(p.next_at(), Some(30));
    }

    #[test]
    fn retry_delay_never_overflows() {
        assert_eq!(retry_delay_secs(0), 60);
        assert_eq!(retry_delay_secs(1), 60);
        assert_eq!(retry_delay_secs(u32::MAX), 30 * 60);
    }
}
