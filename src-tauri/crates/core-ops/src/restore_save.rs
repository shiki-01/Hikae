// 元に戻した後の保存（設計書 4.2 手順 5、設定 `auto_save_after_restore`）。
//
// 元に戻す処理（復元点 → `restore --source`）が成功したあとで、既存の保存（`operations::save`）を
// そのまま呼ぶ。保存はサイズ検査・復元点・メモのトレーラーを通常の保存と同じ経路で行う。
// 呼び出し側（`app`）が同じ直列キューの 1 回の実行の中で呼ぶため、戻す操作と保存の間に
// 別の状態変更は入らない（不変条件 7）。
//
// - 保存で大きいファイルの確認が必要になった場合は、**保存せず**（何も変更せず）`NeedsSizeDecision` を返す。
//   戻す操作は成功しており、画面が通常の保存の流れ（サイズ確認）に任せる
// - 保存が失敗しても、戻す操作は成功として扱う（`Failed`）。取り消し用の復元点は返す
// - 保存は作業フォルダの未保存の変更をすべて含む（ステージングは常に全件。設計書 4.1）

use crate::models::*;
use crate::operations;
use crate::pc_name::Meta;
use crate::restore_file;
use crate::size_check::{SaveOptions, SizeLimits};
use core_git::GitRunner;
use std::path::Path;
use time::OffsetDateTime;

/// 戻したあとに保存する。`memo` が空なら保存しない。
fn save_after_restore(
    runner: &GitRunner,
    repo: &Path,
    memo: &str,
    limits: SizeLimits,
    now: OffsetDateTime,
    meta: Meta,
) -> AfterRestoreSave {
    if memo.trim().is_empty() {
        return AfterRestoreSave::NotRequested;
    }
    let options = SaveOptions {
        limits,
        accept_warned: false,
        exclude: Vec::new(),
    };
    match operations::save(runner, repo, memo, &options, now, meta) {
        Ok(SaveOutcome::Saved { commit, .. }) => AfterRestoreSave::Saved { commit },
        Ok(SaveOutcome::NothingToSave) => AfterRestoreSave::NothingToSave,
        Ok(SaveOutcome::NeedsSizeDecision(_)) => AfterRestoreSave::NeedsSizeDecision,
        Err(_) => AfterRestoreSave::Failed,
    }
}

/// 指定の時点へ全体を戻し、`save_memo` があれば続けて保存する。
pub(crate) fn restore_and_save(
    runner: &GitRunner,
    repo: &Path,
    target_commit: &str,
    save_memo: Option<&str>,
    limits: SizeLimits,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<RestoreAndSave, OpsError> {
    let undo_ref = operations::restore(runner, repo, target_commit, now, meta)?;
    let save = match save_memo {
        Some(memo) => save_after_restore(runner, repo, memo, limits, now, meta),
        None => AfterRestoreSave::NotRequested,
    };
    Ok(RestoreAndSave { undo_ref, save })
}

/// 指定時点の 1 ファイルを戻し、戻せたときだけ、`save_memo` があれば続けて保存する。
#[allow(clippy::too_many_arguments)]
pub(crate) fn restore_file_and_save(
    runner: &GitRunner,
    repo: &Path,
    commit: &str,
    path: &str,
    save_memo: Option<&str>,
    limits: SizeLimits,
    now: OffsetDateTime,
    meta: Meta,
) -> Result<RestoreFileAndSave, OpsError> {
    let outcome = restore_file::restore_file(runner, repo, commit, path, now, meta)?;
    let save = match (&outcome, save_memo) {
        (RestoreFileOutcome::Restored { .. }, Some(memo)) => {
            save_after_restore(runner, repo, memo, limits, now, meta)
        }
        _ => AfterRestoreSave::NotRequested,
    };
    Ok(RestoreFileAndSave { outcome, save })
}

/// 設定「元に戻した後に自動で保存」（`enabled`）と画面が渡したメモから、続けて保存するメモを決める。
/// 設定がオフ、またはメモが空（空白だけを含む）なら保存しない。
pub fn memo_for_auto_save(enabled: bool, memo: Option<&str>) -> Option<String> {
    memo.map(str::trim)
        .filter(|m| enabled && !m.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_is_used_only_when_the_setting_is_on_and_the_memo_is_not_blank() {
        assert_eq!(
            memo_for_auto_save(true, Some("  10/4 の状態に戻しました ")).as_deref(),
            Some("10/4 の状態に戻しました")
        );
        assert_eq!(memo_for_auto_save(false, Some("戻しました")), None);
        assert_eq!(memo_for_auto_save(true, Some("   ")), None);
        assert_eq!(memo_for_auto_save(true, None), None);
    }
}
