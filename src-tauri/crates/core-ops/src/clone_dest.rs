// 取得先フォルダの検査。既存のファイルを巻き込まないよう、空でないフォルダへは取得しない。

use std::path::Path;

/// 取得先として使えない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneDestinationError {
    /// 同名のファイルなど、フォルダではないものがある
    NotADirectory,
    /// フォルダが空ではない
    NotEmpty,
    /// フォルダの中身を確認できない（権限など）
    Unreadable,
}

/// 取得先に使えるか確認する。存在しない場所、または空のフォルダなら Ok。
/// 確認だけで、フォルダの作成・変更・削除は行わない。
pub fn check_clone_destination(dest: &Path) -> Result<(), CloneDestinationError> {
    if !dest.exists() {
        return Ok(());
    }
    if !dest.is_dir() {
        return Err(CloneDestinationError::NotADirectory);
    }
    match std::fs::read_dir(dest) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                Err(CloneDestinationError::NotEmpty)
            } else {
                Ok(())
            }
        }
        Err(_) => Err(CloneDestinationError::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_missing_and_empty_but_not_others() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        assert_eq!(check_clone_destination(&tmp.path().join("new")), Ok(()));
        assert_eq!(check_clone_destination(tmp.path()), Ok(()));

        std::fs::write(tmp.path().join("a.txt"), "x")?;
        assert_eq!(
            check_clone_destination(tmp.path()),
            Err(CloneDestinationError::NotEmpty)
        );
        assert_eq!(
            check_clone_destination(&tmp.path().join("a.txt")),
            Err(CloneDestinationError::NotADirectory)
        );
        Ok(())
    }
}
