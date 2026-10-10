// 拡張ホストのテスト。
//
// - 同値性: ネイティブの `extract_*` と、ビルド済みの WASM 抽出器（組み込みの拡張）の結果が一致する。
//   WASM が無いときは、ビルド手順を示して失敗する（スキップしない）
// - 制限・権限: テスト専用の WAT（`wat` でコンパイル）で、時間・メモリ・stdout・stdin の上限と、
//   WASI の呼び出し（preopen・環境変数・ソケット）が与えられていないことを確かめる
// - 登録: 一時フォルダの拡張で、不正なマニフェスト・欠けた wasm・重複が拒否されることを確かめる

use super::manifest::{self, MANIFEST_MAX_BYTES};
use super::runtime;
use super::*;
use crate::extension::{error_from_word, error_word};
use crate::test_support::{
    document, docx_of, num, para, pptx_of, pptx_para, pptx_presentation, pptx_rels, pptx_slide,
    row, text, xlsx_of, zip_of,
};
use crate::{extract_docx, extract_pptx, extract_xlsx, Limits, PreviewError};
use std::sync::OnceLock;
use std::time::Instant;

/// 組み込みの拡張（ビルド済みの wasm を含むフォルダ）
fn builtin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("app")
        .join("resources")
        .join("extensions")
        .join("hikae.builtin.office")
}

/// 組み込みの拡張を登録済みのホスト（モジュールのコンパイルは 1 回だけ行う）
fn builtin_host() -> &'static ExtensionHost {
    static HOST: OnceLock<ExtensionHost> = OnceLock::new();
    HOST.get_or_init(|| {
        let dir = builtin_dir();
        assert!(
            dir.join("extract-docx.wasm").is_file(),
            "組み込みの抽出器（wasm）が無い。`node scripts/build-extensions.mjs` を実行してから `cargo test` する"
        );
        let mut host = ExtensionHost::new();
        if let Err(err) = host.register_dir(&dir) {
            panic!("組み込みの拡張を登録できない: {err}");
        }
        host
    })
}

/// ネイティブの失敗を、WASM 側の語彙を通した種類に直す（`pdf` と同じく、NotZip と Unsupported は同じ語）
fn over_the_wire(err: PreviewError) -> PreviewError {
    error_from_word(error_word(err)).unwrap_or_else(|| panic!("語彙に無い失敗: {err:?}"))
}

/// 結果を比べる。失敗は語彙を通した種類で比べる
fn assert_same<T: PartialEq + std::fmt::Debug>(
    name: &str,
    native: Result<T, PreviewError>,
    wasm: Result<T, PreviewError>,
) {
    match native {
        Ok(value) => assert!(wasm == Ok(value), "{name}: WASM の結果が違う: {wasm:?}"),
        Err(err) => assert!(
            wasm == Err(over_the_wire(err)),
            "{name}: WASM の失敗が違う（ネイティブは {err:?}、WASM は {wasm:?}）"
        ),
    }
}

// ---- 同値性 ----

fn docx_cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty body", docx_of(&document(""))),
        (
            "paragraphs and empty ones",
            docx_of(&document(&format!("{}<w:p/>{}", para("a"), para("")))),
        ),
        (
            "runs, tabs and breaks",
            docx_of(&document(
                r#"<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>"#,
            )),
        ),
        (
            "table cells in order",
            docx_of(&document(&format!(
                r#"{}<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>{}"#,
                para("before"),
                para("c1"),
                para("c2"),
                para("after")
            ))),
        ),
        (
            "escapes and character references",
            docx_of(&document(
                r#"<w:p><w:r><w:t>A &amp; B &lt;x&gt; &#x41;&#66;</w:t></w:r></w:p>"#,
            )),
        ),
        (
            "deleted text and fallback",
            docx_of(&document(
                r#"<w:p><w:del><w:r><w:delText>gone</w:delText></w:r></w:del><w:r><w:t>kept</w:t></w:r></w:p><w:p><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="wps"><w:r><w:t>choice</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>fallback</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>"#,
            )),
        ),
    ]
}

#[test]
fn docx_output_matches_the_native_extraction() {
    let host = builtin_host();
    for (name, bytes) in docx_cases() {
        let native = extract_docx(&bytes, &Limits::default());
        assert_same(name, native, host.docx_paragraphs(&bytes));
    }
}

#[test]
fn xlsx_output_matches_the_native_extraction() {
    let host = builtin_host();
    let books: Vec<(&str, Vec<u8>)> = vec![
        (
            "values of three kinds",
            xlsx_of(&[(
                "Sheet1",
                &row(
                    1,
                    &(text("A1", "name") + &num("B1", "120") + &num("C1", "0.5")),
                ),
            )]),
        ),
        (
            "sheets keep the workbook order",
            xlsx_of(&[
                ("Beta", &row(1, &text("A1", "b"))),
                ("Alpha", &row(1, &text("A1", "a"))),
            ]),
        ),
        (
            "booleans, errors and stored formula results",
            xlsx_of(&[(
                "S",
                &row(
                    1,
                    r#"<c r="A1" t="b"><v>1</v></c><c r="B1" t="e"><v>#DIV/0!</v></c><c r="C1"><f>1+1</f><v>2</v></c>"#,
                ),
            )]),
        ),
        (
            "sparse cells far apart",
            xlsx_of(&[(
                "Far",
                &(row(1, &num("A1", "1")) + &row(50, &num("Z50", "-3.25"))),
            )]),
        ),
        ("empty sheet", xlsx_of(&[("Empty", "")])),
    ];
    for (name, bytes) in books {
        let native = extract_xlsx(&bytes, &Limits::default());
        assert_same(name, native, host.xlsx_sheets(&bytes));
    }
}

#[test]
fn pptx_output_matches_the_native_extraction() {
    let host = builtin_host();
    // 表示順は presentation.xml の並び（部品名の順と逆）
    let ordered = pptx_of(&[
        ("ppt/presentation.xml", pptx_presentation(&["rId2", "rId1"])),
        (
            "ppt/_rels/presentation.xml.rels",
            pptx_rels(&[("rId1", "slides/slide1.xml"), ("rId2", "slides/slide2.xml")]),
        ),
        ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("first"))),
        (
            "ppt/slides/slide2.xml",
            pptx_slide(&(pptx_para("second") + &pptx_para("   "))),
        ),
    ]);
    let no_rels = pptx_of(&[
        ("ppt/slides/slide1.xml", pptx_slide(&pptx_para("a"))),
        ("ppt/slides/slide2.xml", pptx_slide("")),
    ]);
    for (name, bytes) in [("presentation order", ordered), ("name order", no_rels)] {
        let native = extract_pptx(&bytes, &Limits::default());
        assert_same(name, native, host.pptx_slides(&bytes));
    }
}

/// 暗号化相当（zip のエントリの暗号化フラグを立てる）。ローカルヘッダと中央ディレクトリの両方を書き換える
fn set_encrypted_flags(zip: &mut [u8]) {
    for signature in [b"PK\x03\x04", b"PK\x01\x02"] {
        let offset = if signature[2] == 0x03 { 6 } else { 8 };
        let mut i = 0;
        while i + 10 <= zip.len() {
            if &zip[i..i + 4] == signature {
                zip[i + offset] |= 1;
            }
            i += 1;
        }
    }
}

#[test]
fn errors_match_the_native_kind() {
    let host = builtin_host();
    let valid = docx_of(&document(&para("x")));
    let mut truncated = valid.clone();
    truncated.truncate(valid.len() / 2);
    let mut encrypted = valid.clone();
    set_encrypted_flags(&mut encrypted);
    let broken = [b"PK\x03\x04".as_slice(), &[0xFF; 40]].concat();

    for (name, bytes) in [
        ("not a zip", b"hello, this is not a zip".to_vec()),
        ("broken zip", broken),
        ("truncated zip", truncated),
        ("encrypted entries", encrypted),
    ] {
        let native = extract_docx(&bytes, &Limits::default());
        assert!(
            native.is_err(),
            "{name}: ネイティブが失敗しない前提が崩れた"
        );
        assert_same(name, native, host.docx_paragraphs(&bytes));
    }

    let mut encrypted_book = xlsx_of(&[("S", &row(1, &num("A1", "1")))]);
    set_encrypted_flags(&mut encrypted_book);
    assert_same(
        "encrypted xlsx",
        extract_xlsx(&encrypted_book, &Limits::default()),
        host.xlsx_sheets(&encrypted_book),
    );
}

#[test]
fn an_input_over_the_native_limit_is_too_large_on_both_sides() {
    let host = builtin_host();
    // ネイティブの入力の上限（50 MiB）と同じ大きさ。先頭が zip でなくても、大きさで先に止まる
    let big = vec![0u8; 50 * 1024 * 1024 + 1];
    assert_same(
        "input over 50 MiB",
        extract_docx(&big, &Limits::default()),
        host.docx_paragraphs(&big),
    );
}

#[test]
fn too_many_paragraphs_is_too_large_on_both_sides() {
    let host = builtin_host();
    // 段落の上限（既定の 100,000）を 1 つ超える
    let body = "<w:p/>".repeat(Limits::default().max_paragraphs + 1);
    let bytes = docx_of(&document(&body));
    assert_same(
        "paragraph limit",
        extract_docx(&bytes, &Limits::default()),
        host.docx_paragraphs(&bytes),
    );
}

#[test]
fn too_many_cells_is_too_large_on_both_sides() {
    let host = builtin_host();
    // 値のあるセルの上限（既定の 200,000）を 1 つ超える。1 行に 1 セルずつ（矩形の面積は上限内）
    let count = Limits::default().max_cells + 1;
    let body: String = (1..=count)
        .map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>1</v></c></row>"#))
        .collect();
    let bytes = xlsx_of(&[("Big", &body)]);
    assert_same(
        "cell limit",
        extract_xlsx(&bytes, &Limits::default()),
        host.xlsx_sheets(&bytes),
    );
}

#[test]
fn too_many_slides_is_too_large_on_both_sides() {
    let host = builtin_host();
    let count = Limits::default().max_slides + 1;
    let parts: Vec<(String, String)> = (1..=count)
        .map(|n| {
            (
                format!("ppt/slides/slide{n}.xml"),
                pptx_slide(&pptx_para("s")),
            )
        })
        .collect();
    let refs: Vec<(&str, String)> = parts.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
    let bytes = pptx_of(&refs);
    assert_same(
        "slide limit",
        extract_pptx(&bytes, &Limits::default()),
        host.pptx_slides(&bytes),
    );
}

#[test]
fn extractors_are_looked_up_by_file_type_regardless_of_dot_and_case() {
    let host = builtin_host();
    for name in [".docx", "docx", ".DOCX", "Docx"] {
        assert!(
            host.extractors_for(name).is_some(),
            "{name} should be found"
        );
    }
    assert!(host.extractors_for(".pdf").is_none());
    assert!(host.extractors_for("doc").is_none());
    let info = host.info();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].id, "hikae.builtin.office");
    assert_eq!(info[0].version, "0.1.0");
}

// ---- 制限・権限（テスト専用の WASM） ----

/// テスト用の拡張を登録する。`(ファイル種別, WAT)` の組を、宣言の順に 1 つずつ抽出器にする（出力は paragraphs）
fn host_with(limits: RuntimeLimits, extractors: &[(&str, &str)]) -> ExtensionHost {
    let decls: Vec<serde_json::Value> = extractors
        .iter()
        .enumerate()
        .map(|(i, (ext, _))| {
            serde_json::json!({
                "fileTypes": [format!(".{ext}")],
                "output": "paragraphs",
                "entry": format!("x{i}.wasm"),
            })
        })
        .collect();
    let json = serde_json::json!({
        "id": "test.limits",
        "name": "テスト",
        "version": "0.0.1",
        "apiVersion": "1",
        "contributes": { "extractors": decls },
    });
    let manifest = manifest::parse(json.to_string().as_bytes()).unwrap_or_else(|e| {
        panic!("テストのマニフェストが不正: {e}");
    });
    let wasm: Vec<Vec<u8>> = extractors
        .iter()
        .map(|(_, wat)| wat::parse_str(wat).unwrap_or_else(|e| panic!("WAT が不正: {e}")))
        .collect();
    let mut host = ExtensionHost::with_limits(limits);
    if let Err(err) = host.register(manifest, wasm) {
        panic!("テストの拡張を登録できない: {err}");
    }
    host
}

/// WAT をバイナリにする（不正な WAT はテストの前提の誤り）
fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap_or_else(|e| panic!("WAT が不正: {e}"))
}

/// バイト列を WAT のデータ（`\hh` の並び）にする
fn wat_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// stdout に固定の JSON を書いて、正常に終了する WAT
fn fixed_output(json: &str) -> String {
    format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "{data}")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 1024))
    (i32.store (i32.const 4) (i32.const {len}))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8))))
)"#,
        data = wat_bytes(json.as_bytes()),
        len = json.len()
    )
}

const PARAGRAPHS_OK: &str = r#"{"output":"paragraphs","paragraphs":["ok"]}"#;

/// 無限ループ
const SPIN: &str = r#"(module (func (export "_start") (loop $l (br $l))))"#;

/// 16 MiB ずつメモリを伸ばし続ける。拒否されたら trap する
const HOG: &str = r#"(module
  (memory (export "memory") 1)
  (func (export "_start")
    (loop $l
      (if (i32.eq (memory.grow (i32.const 256)) (i32.const -1)) (then unreachable))
      (br $l))))"#;

/// 何もせず trap する（パニックに相当）
const TRAP: &str = r#"(module (func (export "_start") unreachable))"#;

/// 64 KiB ずつ 300 回 stdout に書く（約 19.5 MiB）。書けなくなったら終わる
const FLOOD: &str = r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 2)
  (func (export "_start")
    (local $i i32)
    (i32.store (i32.const 0) (i32.const 1024))
    (i32.store (i32.const 4) (i32.const 65536))
    (block $done
      (loop $l
        (br_if $done (i32.ge_u (local.get $i) (i32.const 300)))
        (br_if $done (i32.ne (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)) (i32.const 0)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $l))))
)"#;

/// WASI の呼び出しを試して、結果（errno など）を 24 バイトの i32 として stdout に書く。
/// 2048: path_open、2052: environ_sizes_get の errno、2056: 環境変数の数、2060: sock_accept、
/// 2064: sock_recv、2068: sock_send
const PROBE: &str = r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $environ_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_accept" (func $sock_accept (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_recv" (func $sock_recv (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_send" (func $sock_send (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    (i32.store (i32.const 2048) (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 512)))
    (i32.store (i32.const 2052) (call $environ_sizes_get (i32.const 520) (i32.const 524)))
    (i32.store (i32.const 2056) (i32.load (i32.const 520)))
    (i32.store (i32.const 2060) (call $sock_accept (i32.const 3) (i32.const 0) (i32.const 528)))
    (i32.store (i32.const 2064) (call $sock_recv (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 532) (i32.const 536)))
    (i32.store (i32.const 2068) (call $sock_send (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 540)))
    (i32.store (i32.const 2032) (i32.const 2048))
    (i32.store (i32.const 2036) (i32.const 24))
    (drop (call $fd_write (i32.const 1) (i32.const 2032) (i32.const 1) (i32.const 2040))))
)"#;

fn limits_with_timeout(timeout: Duration) -> RuntimeLimits {
    RuntimeLimits {
        timeout,
        ..RuntimeLimits::default()
    }
}

#[test]
fn a_spinning_extractor_is_stopped_at_the_time_limit() {
    let host = host_with(
        limits_with_timeout(Duration::from_millis(300)),
        &[("spin", SPIN)],
    );
    let extractor = host
        .extractors_for(".spin")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    let started = Instant::now();
    let result = host.extract(&extractor, b"");
    assert_eq!(result.err(), Some(ExtractError::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "時間の上限で止まっていない: {:?}",
        started.elapsed()
    );
}

#[test]
fn memory_past_the_limit_is_refused_and_reported_as_out_of_memory() {
    let host = host_with(RuntimeLimits::default(), &[("hog", HOG)]);
    let extractor = host
        .extractors_for(".hog")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, b"").err(),
        Some(ExtractError::OutOfMemory)
    );
}

#[test]
fn a_trapping_extractor_does_not_stop_the_host() {
    let host = host_with(
        RuntimeLimits::default(),
        &[("trap", TRAP), ("ok", &fixed_output(PARAGRAPHS_OK))],
    );
    let trap = host
        .extractors_for(".trap")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    let ok = host
        .extractors_for(".ok")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&trap, b"").err(),
        Some(ExtractError::ExtractorFailed)
    );
    // 同じホストで、次の抽出が成功する
    assert_eq!(
        host.extract(&ok, b"").ok(),
        Some(ExtractOutput::Paragraphs(vec!["ok".to_owned()]))
    );
    assert_eq!(
        host.extract(&trap, b"").err(),
        Some(ExtractError::ExtractorFailed)
    );
}

#[test]
fn a_timed_out_extractor_does_not_stop_the_next_call() {
    let host = host_with(
        limits_with_timeout(Duration::from_millis(300)),
        &[("spin", SPIN), ("ok", &fixed_output(PARAGRAPHS_OK))],
    );
    let spin = host
        .extractors_for(".spin")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    let ok = host
        .extractors_for(".ok")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(host.extract(&spin, b"").err(), Some(ExtractError::Timeout));
    assert!(host.extract(&ok, b"").is_ok());
}

#[test]
fn output_over_the_stdout_limit_is_too_large() {
    let host = host_with(RuntimeLimits::default(), &[("flood", FLOOD)]);
    let extractor = host
        .extractors_for(".flood")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, b"").err(),
        Some(ExtractError::TooLarge)
    );
}

#[test]
fn input_over_the_stdin_limit_is_refused_before_running() {
    let limits = RuntimeLimits {
        stdin_bytes: 16,
        ..RuntimeLimits::default()
    };
    let host = host_with(limits, &[("ok", &fixed_output(PARAGRAPHS_OK))]);
    let extractor = host
        .extractors_for(".ok")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, &[0u8; 17]).err(),
        Some(ExtractError::TooLarge)
    );
    assert!(host.extract(&extractor, &[0u8; 16]).is_ok());
}

#[test]
fn a_non_zero_exit_reports_the_error_word_from_stdout() {
    let wat = format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "{data}")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 1024))
    (i32.store (i32.const 4) (i32.const {len}))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
    (call $proc_exit (i32.const 1))))"#,
        data = wat_bytes(br#"{"error":"encrypted"}"#),
        len = br#"{"error":"encrypted"}"#.len()
    );
    let host = host_with(RuntimeLimits::default(), &[("enc", &wat)]);
    let extractor = host
        .extractors_for(".enc")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, b"").err(),
        Some(ExtractError::Failed(PreviewError::Encrypted))
    );
}

#[test]
fn a_success_output_of_the_wrong_kind_is_rejected() {
    // 宣言は paragraphs だが、出力はセル
    let cells = r#"{"output":"cells","sheets":[]}"#;
    let host = host_with(RuntimeLimits::default(), &[("bad", &fixed_output(cells))]);
    let extractor = host
        .extractors_for(".bad")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, b"").err(),
        Some(ExtractError::ExtractorFailed)
    );
}

#[test]
fn wasi_gives_no_files_no_environment_and_no_sockets() {
    // 実行の入口で、WASI の呼び出しの結果を直接読む（stdout の 24 バイト）
    let engine = runtime::new_engine().unwrap_or_else(|e| panic!("Engine: {e}"));
    let module = runtime::compile(&engine, &wasm(PROBE)).unwrap_or_else(|e| panic!("compile: {e}"));
    let stdout = runtime::run(&engine, &module, &RuntimeLimits::default(), b"")
        .unwrap_or_else(|e| panic!("run: {e}"));
    assert_eq!(stdout.len(), 24, "プローブの結果の長さ");
    let word =
        |i: usize| i32::from_le_bytes([stdout[i], stdout[i + 1], stdout[i + 2], stdout[i + 3]]);
    // preopen が無いため、ディレクトリの fd 3 は無効（EBADF = 8）
    assert_eq!(word(0), 8, "path_open（ファイルを開けない）");
    // 環境変数は与えない。取得は成功するが、数は 0
    assert_eq!(word(4), 0, "environ_sizes_get の errno");
    assert_eq!(word(8), 0, "環境変数の数");
    // ソケットは無い（EBADF）
    assert_eq!(word(12), 8, "sock_accept");
    assert_eq!(word(16), 8, "sock_recv");
    assert_eq!(word(20), 8, "sock_send");
}

// ---- 登録（マニフェストと wasm の検査） ----

#[test]
fn a_duplicate_id_and_a_claimed_file_type_are_rejected() {
    let ok = fixed_output(PARAGRAPHS_OK);
    let mut host = host_with(RuntimeLimits::default(), &[("one", &ok)]);
    // 同じ ID
    let again = manifest::parse(
        br#"{"id":"test.limits","name":"x","version":"1.0.0","apiVersion":"1",
            "contributes":{"extractors":[{"fileTypes":[".two"],"output":"paragraphs","entry":"a.wasm"}]}}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        host.register(again, vec![wasm("(module)")]).err(),
        Some(ManifestError::DuplicateId)
    );
    // 別の ID だが、同じ拡張子
    let clash = manifest::parse(
        br#"{"id":"test.other","name":"x","version":"1.0.0","apiVersion":"1",
            "contributes":{"extractors":[{"fileTypes":[".one"],"output":"paragraphs","entry":"a.wasm"}]}}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        host.register(clash, vec![wasm("(module)")]).err(),
        Some(ManifestError::FileTypeConflict)
    );
    assert_eq!(host.info().len(), 1, "失敗した拡張は登録されない");
}

#[test]
fn a_module_that_does_not_compile_is_rejected() {
    let mut host = ExtensionHost::new();
    let manifest = manifest::parse(
        br#"{"id":"test.bad","name":"x","version":"1.0.0","apiVersion":"1",
            "contributes":{"extractors":[{"fileTypes":[".bad"],"output":"paragraphs","entry":"a.wasm"}]}}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        host.register(manifest, vec![b"\0asm not really".to_vec()])
            .err(),
        Some(ManifestError::ModuleInvalid)
    );
}

/// 一時フォルダに拡張を作る（マニフェストと、任意のファイル）
fn extension_folder(manifest: &[u8], files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join(MANIFEST_FILE), manifest)
        .unwrap_or_else(|e| panic!("write: {e}"));
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap_or_else(|e| panic!("write: {e}"));
    }
    dir
}

fn manifest_with_entry(entry: &str) -> String {
    format!(
        r#"{{"id":"test.folder","name":"x","version":"1.0.0","apiVersion":"1",
            "contributes":{{"extractors":[{{"fileTypes":[".f"],"output":"paragraphs","entry":"{entry}"}}]}}}}"#
    )
}

#[test]
fn a_folder_registers_when_its_manifest_and_wasm_are_valid() {
    let bytes = wasm(&fixed_output(PARAGRAPHS_OK));
    let dir = extension_folder(
        manifest_with_entry("x.wasm").as_bytes(),
        &[("x.wasm", &bytes)],
    );
    let mut host = ExtensionHost::new();
    let id = host
        .register_dir(dir.path())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(id.as_str(), "test.folder");
    let extractor = host
        .extractors_for("f")
        .unwrap_or_else(|| panic!("抽出器が無い"));
    assert_eq!(
        host.extract(&extractor, b"").ok(),
        Some(ExtractOutput::Paragraphs(vec!["ok".to_owned()]))
    );
}

#[test]
fn a_folder_with_a_bad_manifest_or_a_missing_wasm_is_not_registered() {
    let mut host = ExtensionHost::new();

    // マニフェストが無い
    let empty = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    assert_eq!(
        host.register_dir(empty.path()).err(),
        Some(ManifestError::Unreadable)
    );

    // マニフェストが 64 KiB を超える
    let big = vec![b' '; MANIFEST_MAX_BYTES as usize + 1];
    let dir = extension_folder(&big, &[]);
    assert_eq!(
        host.register_dir(dir.path()).err(),
        Some(ManifestError::TooLarge)
    );

    // 宣言の wasm が無い
    let dir = extension_folder(manifest_with_entry("missing.wasm").as_bytes(), &[]);
    assert_eq!(
        host.register_dir(dir.path()).err(),
        Some(ManifestError::EntryMissing)
    );

    // entry が拡張フォルダの外を指す（`..`）。宣言の時点で拒否する
    let dir = extension_folder(manifest_with_entry("../escape.wasm").as_bytes(), &[]);
    assert_eq!(
        host.register_dir(dir.path()).err(),
        Some(ManifestError::InvalidEntry)
    );

    // 形式の違う id
    let dir = extension_folder(
        br#"{"id":"Bad_ID","name":"x","version":"1.0.0","apiVersion":"1"}"#,
        &[],
    );
    assert_eq!(
        host.register_dir(dir.path()).err(),
        Some(ManifestError::InvalidId)
    );

    assert!(host.info().is_empty(), "どの失敗も登録を残さない");
}

#[test]
fn a_zip_is_never_treated_as_an_extension_module() {
    // 抽出器の入力（zip）を wasm として登録しても、wasm ではないので拒否する
    let mut host = ExtensionHost::new();
    let manifest = manifest::parse(
        br#"{"id":"test.zip","name":"x","version":"1.0.0","apiVersion":"1",
            "contributes":{"extractors":[{"fileTypes":[".z"],"output":"paragraphs","entry":"a.wasm"}]}}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let not_wasm = zip_of(&[("a", b"b")], false);
    assert_eq!(
        host.register(manifest, vec![not_wasm]).err(),
        Some(ManifestError::ModuleInvalid)
    );
}
