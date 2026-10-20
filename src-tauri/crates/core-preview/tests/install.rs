// 利用者の追加フォルダの取り込み・一覧からの除外の統合テスト（設計書 11.9）。
// 一時フォルダに実ファイルを作って確かめる。wasm は中身を使わないため、検査用のダミーを置く
// （取り込みはコンパイルしない。登録は app の起動時に行う）。

use core_preview::{
    install_from_folder, move_to_removed, read_summary, InstallError, ManifestError,
    INSTALL_MAX_BYTES, INSTALL_MAX_FILES,
};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const NOW: &str = "2026-10-20T10:00:00+09:00";

/// 表示の入口 1 つと、抽出器 1 つを持つマニフェスト
fn manifest(id: &str, name: &str) -> String {
    format!(
        r#"{{
  "id": "{id}",
  "name": "{name}",
  "version": "1.2.3",
  "apiVersion": "1",
  "contributes": {{
    "renderers": [{{ "fileTypes": [".txt"], "modes": ["view"], "entry": "ui/index.html" }}],
    "extractors": [{{ "fileTypes": [".dat"], "output": "paragraphs", "entry": "extract.wasm" }}]
  }},
  "permissions": []
}}"#
    )
}

/// 正しい拡張フォルダを作る（ui/index.html と extract.wasm を含む）
fn make_extension(dir: &Path, id: &str) -> TestResult {
    fs::create_dir_all(dir.join("ui"))?;
    fs::write(dir.join("extension.json"), manifest(id, "テスト拡張"))?;
    fs::write(dir.join("ui/index.html"), "<p>表示</p>")?;
    fs::write(dir.join("extract.wasm"), b"\0asm dummy")?;
    Ok(())
}

/// フォルダの中身の 1 件（相対パスと内容。フォルダは内容が空で、パスは `/` で終わる）
type Entry = (String, Vec<u8>);

/// フォルダの中身（相対パスと内容）を、順序を揃えて並べる。変更の検出に使う
fn snapshot(root: &Path) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut Vec<Entry>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.push((format!("{rel}/"), Vec::new()));
                walk(root, &path, out)?;
            } else {
                out.push((rel, fs::read(&path)?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

/// 利用者の拡張フォルダ（`extensions/`）と、取り込み元のフォルダを作る
fn setup() -> Result<(tempfile::TempDir, PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let user_dir = tmp.path().join("extensions");
    let src = tmp.path().join("source");
    fs::create_dir_all(&user_dir)?;
    make_extension(&src, "example.tool")?;
    Ok((tmp, user_dir, src))
}

#[test]
fn install_copies_the_folder_and_leaves_the_source_unchanged() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    let before = snapshot(&src)?;

    let installed = install_from_folder(&src, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    assert_eq!(installed.id, "example.tool");
    assert_eq!(installed.name, "テスト拡張");
    assert_eq!(installed.version, "1.2.3");

    // コピー先は、元と同じ中身（ui と wasm を含む）
    let dest = user_dir.join("example.tool");
    assert_eq!(snapshot(&dest)?, before);
    // 元のフォルダは変わらない
    assert_eq!(snapshot(&src)?, before);
    Ok(())
}

#[test]
fn installing_the_same_id_twice_is_rejected_and_keeps_the_first() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    install_from_folder(&src, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    let first = snapshot(&user_dir.join("example.tool"))?;

    // 2 つ目は別の内容でも、同じ ID なら拒否する（既存は変えない）
    fs::write(src.join("ui/index.html"), "<p>別の版</p>")?;
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(result.err(), Some(InstallError::DuplicateId));
    assert_eq!(snapshot(&user_dir.join("example.tool"))?, first);
    Ok(())
}

#[test]
fn a_standard_id_is_rejected_without_creating_anything() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    let builtin = vec!["example.tool".to_string()];
    let result = install_from_folder(&src, &user_dir, &builtin, NOW);
    assert_eq!(result.err(), Some(InstallError::BuiltinId));
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn an_invalid_manifest_is_rejected_without_creating_anything() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    // 名前が空（検証で拒否）
    fs::write(src.join("extension.json"), manifest("example.tool", "  "))?;
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::InvalidName))
    );
    // 壊れた JSON
    fs::write(src.join("extension.json"), "{ not json")?;
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::Malformed))
    );
    assert_eq!(fs::read_dir(&user_dir)?.count(), 0);
    Ok(())
}

#[test]
fn a_missing_entry_is_rejected_before_copying() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    // 削除せず、別の名前へ移して「宣言された entry が無い」状態を作る
    fs::rename(src.join("ui/index.html"), src.join("moved.html"))?;
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::EntryMissing))
    );
    assert_eq!(fs::read_dir(&user_dir)?.count(), 0);
    Ok(())
}

#[test]
fn a_folder_with_too_many_files_is_rejected_without_creating_anything() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    // 既存の 3 件（マニフェスト・表示・wasm）に加えて、上限を 1 つ超える
    let extra = INSTALL_MAX_FILES - 3 + 1;
    for i in 0..extra {
        fs::write(src.join(format!("data-{i:05}.txt")), b"x")?;
    }
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(result.err(), Some(InstallError::TooLarge));
    assert_eq!(fs::read_dir(&user_dir)?.count(), 0);
    Ok(())
}

#[test]
fn a_folder_over_the_size_limit_is_rejected() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    // 疎なファイルで上限を超える大きさにする（実際には容量を使わない）
    let big = fs::File::create(src.join("big.bin"))?;
    big.set_len(INSTALL_MAX_BYTES + 1)?;
    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(result.err(), Some(InstallError::TooLarge));
    assert_eq!(fs::read_dir(&user_dir)?.count(), 0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_inside_the_folder_are_not_copied() -> TestResult {
    let (tmp, user_dir, src) = setup()?;
    // フォルダの外のファイルへのリンク。取り込みで追ってはいけない
    let outside = tmp.path().join("secret.txt");
    fs::write(&outside, "外のファイル")?;
    std::os::unix::fs::symlink(&outside, src.join("link.txt"))?;

    install_from_folder(&src, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    let dest = user_dir.join("example.tool");
    assert!(!dest.join("link.txt").exists());
    assert!(!snapshot(&dest)?
        .iter()
        .any(|(rel, _)| rel.contains("link") || rel.contains("secret")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_entry_that_only_exists_as_a_link_is_parked_in_failed() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    // 元では実体がフォルダ内にあるためリンクは通るが、取り込みではリンクを飛ばすため揃わない
    fs::rename(src.join("ui/index.html"), src.join("real.html"))?;
    std::os::unix::fs::symlink(src.join("real.html"), src.join("ui/index.html"))?;

    let result = install_from_folder(&src, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::EntryMissing))
    );
    // 作りかけは消さず、_failed へ退けてある。通常の場所には残らない
    assert!(!user_dir.join("example.tool").exists());
    let failed = user_dir
        .join("_failed")
        .join("example.tool_2026-10-20T10-00-00-09-00");
    assert!(failed.join("extension.json").is_file());
    Ok(())
}

#[test]
fn move_to_removed_renames_the_folder_into_removed() -> TestResult {
    let (_tmp, user_dir, src) = setup()?;
    install_from_folder(&src, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    let before = snapshot(&user_dir.join("example.tool"))?;

    move_to_removed(&user_dir, "example.tool", NOW).map_err(|e| format!("{e:?}"))?;
    assert!(!user_dir.join("example.tool").exists());
    let removed = user_dir
        .join("_removed")
        .join("example.tool_2026-10-20T10-00-00-09-00");
    assert_eq!(snapshot(&removed)?, before);

    // 同じ日時の退避が重なっても、既存を上書きしない（別の名前にする）
    install_from_folder(&src, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    move_to_removed(&user_dir, "example.tool", NOW).map_err(|e| format!("{e:?}"))?;
    assert!(user_dir
        .join("_removed")
        .join("example.tool_2026-10-20T10-00-00-09-00_2")
        .is_dir());
    Ok(())
}

#[test]
fn move_to_removed_rejects_unknown_and_invalid_ids() -> TestResult {
    let (_tmp, user_dir, _src) = setup()?;
    assert_eq!(
        move_to_removed(&user_dir, "no.such", NOW).err(),
        Some(InstallError::NotFound)
    );
    assert_eq!(
        move_to_removed(&user_dir, "../escape", NOW).err(),
        Some(InstallError::InvalidId)
    );
    Ok(())
}

#[test]
fn read_summary_reads_the_manifest_without_checking_entries() -> TestResult {
    let (_tmp, _user_dir, src) = setup()?;
    // 表示の入口が無くても、概要は読める（無効の拡張を一覧に出すため）
    // 削除せず、別の名前へ移して「宣言された entry が無い」状態を作る
    fs::rename(src.join("ui/index.html"), src.join("moved.html"))?;
    let summary = read_summary(&src).map_err(|e| format!("{e:?}"))?;
    assert_eq!(summary.id, "example.tool");
    assert_eq!(summary.name, "テスト拡張");
    assert_eq!(summary.version, "1.2.3");
    // 表示と抽出器の拡張子を、重複なしで並べる
    assert_eq!(
        summary.file_types,
        vec!["txt".to_string(), "dat".to_string()]
    );
    Ok(())
}
