// ファイル監視とコンテンツ変更検知（notify + notify-debouncer-full）、
// 定期的な取り込み・アップロードの実行計画（schedule）、保守作業の実行間隔（maintenance）。

pub mod maintenance;
pub mod schedule;

pub use maintenance::{MaintenanceTimer, DAY_SECS};
pub use schedule::{
    classify_failure, retry_delay_secs, FailureKind, SyncHealth, SyncPlanner, SyncPolicy, SyncTask,
    TaskResult,
};
