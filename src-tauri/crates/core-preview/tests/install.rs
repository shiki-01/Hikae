// 利用者の追加フォルダの取り込み・一覧からの除外の統合テスト（設計書 11.9）。
// 一時フォルダに実ファイルを作って確かめる。wasm は中身を使わないため、検査用のダミーを置く
// （取り込みはコンパイルしない。登録は app の起動時に行う）。

use core_preview::{
    install_from_folder, install_from_zip, move_to_removed, read_summary, InstallError,
    ManifestError, INSTALL_MAX_BYTES, INSTALL_MAX_FILES, ZIP_MAX_ENTRIES, ZIP_MAX_FILE_BYTES,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

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

// ---- zip からの取り込み（設計書 11.9）----

/// zip のエントリ（名前と内容。名前の末尾が `/` のものはディレクトリ）
type ZipEntries = Vec<(String, Vec<u8>)>;

/// 圧縮方式を選んで、エントリから zip を作る（テストの入力として使う）
fn write_zip(path: &Path, entries: &ZipEntries, method: CompressionMethod) -> TestResult {
    let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
    let options = SimpleFileOptions::default().compression_method(method);
    for (name, data) in entries {
        if name.ends_with('/') {
            writer.add_directory(name.as_str(), options)?;
        } else {
            writer.start_file(name.as_str(), options)?;
            writer.write_all(data)?;
        }
    }
    writer.finish()?;
    Ok(())
}

/// 正しい拡張の中身（`prefix` を先頭に付ける。空なら直下）
fn extension_entries(prefix: &str, id: &str) -> ZipEntries {
    vec![
        (
            format!("{prefix}extension.json"),
            manifest(id, "テスト拡張").into_bytes(),
        ),
        (
            format!("{prefix}ui/index.html"),
            "<p>表示</p>".as_bytes().to_vec(),
        ),
        (format!("{prefix}extract.wasm"), b"\0asm dummy".to_vec()),
    ]
}

/// 取り込み元の zip を `tmp` に作り、そのパスを返す
fn zip_of(
    tmp: &Path,
    entries: &ZipEntries,
    method: CompressionMethod,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = tmp.join("source.zip");
    write_zip(&path, entries, method)?;
    Ok(path)
}

#[test]
fn zip_with_manifest_at_the_root_is_installed() -> TestResult {
    let (tmp, user_dir, src) = setup()?;
    // 元と同じ中身を zip にして、同じ中身が取り込まれることを確かめる
    let before = snapshot(&src)?;
    let zip_path = zip_of(
        tmp.path(),
        &extension_entries("", "example.tool"),
        CompressionMethod::Deflated,
    )?;

    let installed =
        install_from_zip(&zip_path, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    assert_eq!(installed.id, "example.tool");
    assert_eq!(installed.name, "テスト拡張");
    assert_eq!(installed.version, "1.2.3");
    assert_eq!(snapshot(&user_dir.join("example.tool"))?, before);
    // 展開先は残さない（_removed へ退けてある）。_staging は空
    assert_eq!(fs::read_dir(user_dir.join("_staging"))?.count(), 0);
    assert!(user_dir.join("_removed").is_dir());
    Ok(())
}

#[test]
fn zip_with_one_folder_at_the_top_uses_that_folder() -> TestResult {
    let (tmp, user_dir, src) = setup()?;
    let before = snapshot(&src)?;
    let zip_path = zip_of(
        tmp.path(),
        &extension_entries("example-tool-src/", "example.tool"),
        CompressionMethod::Deflated,
    )?;

    install_from_zip(&zip_path, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    // フォルダの名前ではなく、中の extension.json のあるフォルダだけを取り込む
    assert_eq!(snapshot(&user_dir.join("example.tool"))?, before);
    assert!(!user_dir.join("example-tool-src").exists());
    Ok(())
}

#[test]
fn zip_with_manifest_in_a_folder_next_to_other_files_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    // 直下に別のファイルがあると、「唯一のフォルダ」ではないため取り込み元が決まらない
    let mut entries = extension_entries("pkg/", "example.tool");
    entries.push(("readme.txt".to_string(), b"x".to_vec()));
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    let result = install_from_zip(&zip_path, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::Unreadable))
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn zip_without_extension_json_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let entries = vec![("ui/index.html".to_string(), b"<p>x</p>".to_vec())];
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    let result = install_from_zip(&zip_path, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::Unreadable))
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn zip_with_an_invalid_manifest_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let mut entries = extension_entries("", "example.tool");
    entries[0].1 = manifest("example.tool", "  ").into_bytes();
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    let result = install_from_zip(&zip_path, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::InvalidName))
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn zip_with_a_standard_id_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let zip_path = zip_of(
        tmp.path(),
        &extension_entries("", "example.tool"),
        CompressionMethod::Deflated,
    )?;
    let builtin = vec!["example.tool".to_string()];
    let result = install_from_zip(&zip_path, &user_dir, &builtin, NOW);
    assert_eq!(result.err(), Some(InstallError::BuiltinId));
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn zip_with_a_duplicate_id_is_rejected_and_keeps_the_first() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let zip_path = zip_of(
        tmp.path(),
        &extension_entries("", "example.tool"),
        CompressionMethod::Deflated,
    )?;
    install_from_zip(&zip_path, &user_dir, &[], NOW).map_err(|e| format!("{e:?}"))?;
    let first = snapshot(&user_dir.join("example.tool"))?;

    // 2 つ目は別の内容でも、同じ ID なら拒否する（既存は変えない）
    let mut other = extension_entries("", "example.tool");
    other[1].1 = "<p>別の版</p>".as_bytes().to_vec();
    let zip_path2 = tmp.path().join("second.zip");
    write_zip(&zip_path2, &other, CompressionMethod::Deflated)?;
    let result = install_from_zip(&zip_path2, &user_dir, &[], NOW);
    assert_eq!(result.err(), Some(InstallError::DuplicateId));
    assert_eq!(snapshot(&user_dir.join("example.tool"))?, first);
    Ok(())
}

#[test]
fn unsafe_entry_names_reject_the_whole_zip() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let bad_names = [
        "../escape.txt",
        "ui/../../escape.txt",
        "/etc/passwd",
        "\\\\server\\share\\x.txt",
        "..\\escape.txt",
        "C:/Windows/x.txt",
        "ui/alt:stream.txt",
        "ui/a\0b.txt",
        "CON.txt",
        "ui/LPT1/x.txt",
        "ui/trailing.",
        "ui//double.txt",
    ];
    for name in bad_names {
        let mut entries = extension_entries("", "example.tool");
        entries.push((name.to_string(), b"x".to_vec()));
        let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
        let result = install_from_zip(&zip_path, &user_dir, &[], NOW);
        assert_eq!(
            result.err(),
            Some(InstallError::ArchiveUnsafe),
            "name: {name:?}"
        );
        // 1 つでも違反があれば、正しいエントリも展開しない
        assert!(!user_dir.join("_staging").exists(), "name: {name:?}");
        assert!(!user_dir.join("example.tool").exists(), "name: {name:?}");
    }
    Ok(())
}

#[test]
fn an_absolute_path_is_rejected_as_unsafe() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let mut entries = extension_entries("", "example.tool");
    entries.push(("/abs/outside.txt".to_string(), b"x".to_vec()));
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::ArchiveUnsafe)
    );
    Ok(())
}

#[test]
fn names_that_differ_only_by_case_are_rejected_as_duplicates() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let mut entries = extension_entries("", "example.tool");
    entries.push(("ui/Extra.txt".to_string(), b"a".to_vec()));
    entries.push(("UI/extra.txt".to_string(), b"b".to_vec()));
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::ArchiveUnsafe)
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlink_entry_rejects_the_whole_zip() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    // 外部属性に S_IFLNK を付けたエントリ（add_symlink）は、リンクとして zip 全体を拒否する
    let path = tmp.path().join("link.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&path)?);
    for (name, data) in extension_entries("", "example.tool") {
        writer.start_file(name.as_str(), SimpleFileOptions::default())?;
        writer.write_all(&data)?;
    }
    writer.add_symlink("ui/link.html", "../../secret", SimpleFileOptions::default())?;
    writer.finish()?;
    assert_eq!(
        install_from_zip(&path, &user_dir, &[], NOW).err(),
        Some(InstallError::ArchiveUnsafe)
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn too_many_entries_are_rejected_before_extracting() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let mut entries = extension_entries("", "example.tool");
    // 上限を 1 つ超えるまで、小さなファイルを足す
    while entries.len() <= ZIP_MAX_ENTRIES {
        let n = entries.len();
        entries.push((format!("data/{n:05}.txt"), b"x".to_vec()));
    }
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Stored)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::TooLarge)
    );
    assert!(!user_dir.join("_staging").exists());
    Ok(())
}

#[test]
fn a_file_over_the_per_file_limit_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let mut entries = extension_entries("", "example.tool");
    // 圧縮しない（Stored）ので、圧縮率の検査より前に大きさで拒否される
    entries.push((
        "big.bin".to_string(),
        vec![0u8; (ZIP_MAX_FILE_BYTES + 1) as usize],
    ));
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Stored)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::TooLarge)
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn a_total_over_the_size_limit_is_rejected() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    // 1 ファイルは 32 MiB 以下だが、合計が 64 MiB を超える（圧縮しないため、中身は実際の大きさ）
    let mut entries = extension_entries("", "example.tool");
    let part = 22 * 1024 * 1024;
    for i in 0..3 {
        entries.push((format!("part-{i}.bin"), vec![1u8; part]));
    }
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Stored)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::TooLarge)
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn a_high_compression_ratio_is_rejected_as_a_bomb() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    // 4 MiB の 0 は、deflate で数 KiB になる（圧縮率が上限を超える）
    let mut entries = extension_entries("", "example.tool");
    entries.push(("zeros.bin".to_string(), vec![0u8; 4 * 1024 * 1024]));
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    assert_eq!(
        install_from_zip(&zip_path, &user_dir, &[], NOW).err(),
        Some(InstallError::ArchiveUnsafe)
    );
    assert!(!user_dir.join("example.tool").exists());
    Ok(())
}

#[test]
fn a_file_that_is_not_a_zip_is_unreadable() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let path = tmp.path().join("not-a-zip.zip");
    fs::write(&path, b"this is not a zip file")?;
    assert_eq!(
        install_from_zip(&path, &user_dir, &[], NOW).err(),
        Some(InstallError::ArchiveUnreadable)
    );
    assert!(!user_dir.join("_staging").exists());
    Ok(())
}

#[test]
fn a_missing_zip_file_is_reported_as_unreadable_source() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    let path = tmp.path().join("missing.zip");
    assert_eq!(
        install_from_zip(&path, &user_dir, &[], NOW).err(),
        Some(InstallError::SourceUnreadable)
    );
    Ok(())
}

#[test]
fn a_zip_missing_a_declared_entry_is_parked_in_failed_without_an_id_folder() -> TestResult {
    let (tmp, user_dir, _src) = setup()?;
    // マニフェストは ui/index.html を宣言するが、zip には入れない
    let mut entries = extension_entries("", "example.tool");
    entries.retain(|(name, _)| name != "ui/index.html");
    let zip_path = zip_of(tmp.path(), &entries, CompressionMethod::Deflated)?;
    let result = install_from_zip(&zip_path, &user_dir, &[], NOW);
    assert_eq!(
        result.err(),
        Some(InstallError::Manifest(ManifestError::EntryMissing))
    );
    // 中途半端な <id> は作らない。展開先は削除せず _failed へ退けてある
    assert!(!user_dir.join("example.tool").exists());
    assert_eq!(fs::read_dir(user_dir.join("_staging"))?.count(), 0);
    let failed: Vec<PathBuf> = fs::read_dir(user_dir.join("_failed"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    assert_eq!(failed.len(), 1);
    assert!(failed[0].join("extension.json").is_file());
    Ok(())
}
