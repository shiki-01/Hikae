// ファイル監視とコンテンツ変更検知（notify + notify-debouncer-full）、および
// 定期的な取り込み・アップロードの実行計画（schedule）。

pub mod schedule;

pub use schedule::{
    classify_failure, retry_delay_secs, FailureKind, SyncHealth, SyncPlanner, SyncPolicy, SyncTask,
    TaskResult,
};
