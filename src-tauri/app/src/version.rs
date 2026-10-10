// 画面の時点 ID を、読み取りに使う rev にする共通の入口（app 層）。
//
// - 時点 ID は `current`・`work`・`this`（HEAD）・`cloud`（MERGE_HEAD）・16 進の短縮 ID。規則は core-ops の
//   `Ops::resolve_point` にあり、ここでは呼び出し側が扱いやすい形（版が無いことを None で表す）に写すだけ
// - app-blob・compare_office・preview_file・diff が、この 2 つの関数だけを使う

use core_ops::{Ops, OpsError};
use std::path::Path;

use crate::AppError;

/// 時点 ID を rev にする。版が無いとき（HEAD や MERGE_HEAD が無い、ID が解決できない）は None。
/// 形の不正な値は `InvalidInput` のまま返す
pub(crate) fn resolve_rev(ops: &Ops, repo: &Path, point: &str) -> Result<Option<String>, OpsError> {
    match ops.resolve_point(repo, point) {
        Ok(rev) => Ok(Some(rev)),
        Err(OpsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// 時点 ID を rev にする。版が無いときも画面にエラーとして返す（片側が無いと比べられない処理用）
pub(crate) fn require_rev(ops: &Ops, repo: &Path, point: &str) -> Result<String, AppError> {
    match resolve_rev(ops, repo, point) {
        Ok(Some(rev)) => Ok(rev),
        Ok(None) => Err(AppError::from_ops_error(OpsError::Io(
            std::io::Error::from(std::io::ErrorKind::NotFound),
        ))),
        Err(e) => Err(AppError::from_ops_error(e)),
    }
}
