// ファイル監視（notify + notify-debouncer-full）と、変更後の静止判定による自動保存の実行計画（snapshot_plan）、
// 定期的な取り込み・アップロードの実行計画（schedule）、保守作業の実行間隔（maintenance）。

pub mod changes;
pub mod ignore_rules;
pub mod maintenance;
pub mod registration;
pub mod schedule;
pub mod snapshot_plan;
pub mod watcher;

pub use changes::{summarize, ChangeBatch, ChangeKind, RawEvent, MAX_REPORTED_PATHS};
pub use ignore_rules::{is_builtin_ignored, IgnoreRules};
pub use maintenance::{MaintenanceTimer, DAY_SECS};
pub use registration::{retain_registered, stale_watches};
pub use schedule::{
    classify_failure, retry_delay_secs, FailureKind, SyncHealth, SyncPlanner, SyncPolicy, SyncTask,
    TaskResult,
};
pub use snapshot_plan::{SnapshotAttempt, SnapshotPlanner, SnapshotPolicy};
pub use watcher::{ProjectWatcher, WatchError, WatchEvent, DEBOUNCE};
