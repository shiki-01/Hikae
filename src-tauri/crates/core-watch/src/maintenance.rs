// 起動時と日次で行う保守作業（復元点の間引き、操作ジャーナルの整理）の実行間隔。
//
// 時刻は呼び出し側が渡す単調増加の秒数（`now`）で扱う純粋なロジック。
// 実際の作業の実行は app 層が担当する。

/// 1 日の秒数
pub const DAY_SECS: u64 = 86_400;

/// 保守作業のタイマー。最初は必ず「実行する時期」で、実行を記録すると間隔が空くまで待つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceTimer {
    interval_secs: u64,
    last_run: Option<u64>,
}

impl MaintenanceTimer {
    /// 起動直後に 1 回、その後は `interval_secs` ごとに実行する
    pub fn new(interval_secs: u64) -> Self {
        MaintenanceTimer {
            interval_secs,
            last_run: None,
        }
    }

    /// 日次（起動時 + 24 時間ごと）
    pub fn daily() -> Self {
        Self::new(DAY_SECS)
    }

    /// 実行する時期か
    pub fn is_due(&self, now: u64) -> bool {
        match self.last_run {
            None => true,
            Some(last) => now.saturating_sub(last) >= self.interval_secs,
        }
    }

    /// 実行したことを記録する（成功・失敗にかかわらず、次は間隔が空いてから）
    pub fn mark_run(&mut self, now: u64) {
        self.last_run = Some(now);
    }

    /// 次に実行する時期までの秒数。いま実行する時期なら 0
    pub fn secs_until_due(&self, now: u64) -> u64 {
        match self.last_run {
            None => 0,
            Some(last) => (last + self.interval_secs).saturating_sub(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_at_start_then_once_per_interval() {
        let mut t = MaintenanceTimer::daily();
        assert!(t.is_due(0));
        assert_eq!(t.secs_until_due(0), 0);

        t.mark_run(5);
        assert!(!t.is_due(5));
        assert!(!t.is_due(5 + DAY_SECS - 1));
        assert_eq!(t.secs_until_due(100), 5 + DAY_SECS - 100);
        assert!(t.is_due(5 + DAY_SECS));
        assert_eq!(t.secs_until_due(5 + DAY_SECS), 0);

        t.mark_run(5 + DAY_SECS + 40);
        assert!(!t.is_due(5 + DAY_SECS + 41));
        assert!(t.is_due(5 + 2 * DAY_SECS + 40));
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_trigger_early() {
        let mut t = MaintenanceTimer::new(100);
        t.mark_run(1_000);
        assert!(!t.is_due(10));
        assert_eq!(t.secs_until_due(10), 1_090);
    }
}
