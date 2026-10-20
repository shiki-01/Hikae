// 拡張の索引の署名検証・形の検証・拡張本体の検証（設計書 11.10）の単体テスト。
// 鍵は固定の seed から作る（秘密鍵をリポジトリに置かない）。ネットワークには触れない。

use base64::Engine as _;
use core_preview::{
    parse_index, verify_download, verify_index_signature, IndexError, INDEX_MAX_BYTES,
    INDEX_MAX_DOWNLOAD_BYTES,
};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SUPPORTED: &[&str] = &["1"];
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// 固定の seed から鍵を作る（テストごとに同じ鍵になる）
fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// 本文に署名し、base64 の文字列にする
fn sign_base64(key: &SigningKey, bytes: &[u8]) -> String {
    let signature = key.sign(bytes);
    base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
}

/// 上限内に収まる、有効な項目 1 件
fn item(id: &str) -> Value {
    json!({
        "id": id,
        "name": "PDF プレビュー",
        "version": "1.0.0",
        "apiVersion": "1",
        "summary": "PDF を表示します",
        "fileTypes": [".pdf"],
        "download": {
            "url": "https://example.com/pdf-preview.zip",
            "sha256": SHA,
            "size": 1024
        },
        "permissions": []
    })
}

/// `schema` と `extensions` から索引の本文を作る
fn index_of(items: &[Value]) -> Vec<u8> {
    let root = json!({ "schema": 1, "extensions": items });
    serde_json::to_vec(&root).expect("索引の JSON を作れる")
}

#[test]
fn a_correctly_signed_index_verifies_and_parses() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let bytes = index_of(&[item("example.pdf-preview")]);
    let sig = sign_base64(&key, &bytes);

    verify_index_signature(&bytes, &sig, &public_key)?;
    let index = parse_index(&bytes, SUPPORTED)?;
    assert_eq!(index.extensions.len(), 1);
    assert_eq!(index.extensions[0].id, "example.pdf-preview");
    assert_eq!(index.extensions[0].download.size, 1024);
    assert_eq!(index.excluded_permissions, 0);
    assert_eq!(index.excluded_api_version, 0);
    Ok(())
}

#[test]
fn one_byte_change_breaks_the_signature() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let bytes = index_of(&[item("example.pdf-preview")]);
    let sig = sign_base64(&key, &bytes);

    let mut tampered = bytes.clone();
    tampered[bytes.len() / 2] ^= 0x01;
    assert_eq!(
        verify_index_signature(&tampered, &sig, &public_key),
        Err(IndexError::SignatureMismatch)
    );
    Ok(())
}

#[test]
fn a_signature_from_another_key_is_rejected() -> TestResult {
    let signer = signing_key(1);
    let other = signing_key(2);
    let bytes = index_of(&[item("example.pdf-preview")]);
    let sig = sign_base64(&signer, &bytes);

    assert_eq!(
        verify_index_signature(&bytes, &sig, &other.verifying_key().to_bytes()),
        Err(IndexError::SignatureMismatch)
    );
    Ok(())
}

#[test]
fn a_signature_that_is_not_base64_is_an_encoding_error() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let bytes = index_of(&[item("example.pdf-preview")]);

    assert_eq!(
        verify_index_signature(&bytes, "!!! not base64 !!!", &public_key),
        Err(IndexError::SignatureEncoding)
    );
    Ok(())
}

#[test]
fn a_signature_of_the_wrong_length_is_a_length_error() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let bytes = index_of(&[item("example.pdf-preview")]);
    let engine = base64::engine::general_purpose::STANDARD;

    for len in [0usize, 63, 65] {
        let sig = engine.encode(vec![0u8; len]);
        assert_eq!(
            verify_index_signature(&bytes, &sig, &public_key),
            Err(IndexError::SignatureLength),
            "length {len} should be rejected"
        );
    }
    Ok(())
}

#[test]
fn a_trailing_newline_after_the_signature_is_accepted() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let bytes = index_of(&[item("example.pdf-preview")]);
    let sig = format!("{}\n", sign_base64(&key, &bytes));

    verify_index_signature(&bytes, &sig, &public_key)?;
    Ok(())
}

#[test]
fn an_index_over_1_mib_is_too_large_for_both_steps() -> TestResult {
    let key = signing_key(1);
    let public_key = key.verifying_key().to_bytes();
    let mut bytes = index_of(&[item("example.pdf-preview")]);
    bytes.resize(INDEX_MAX_BYTES + 1, b' ');
    let sig = sign_base64(&key, &bytes);

    assert_eq!(
        verify_index_signature(&bytes, &sig, &public_key),
        Err(IndexError::TooLarge)
    );
    assert_eq!(parse_index(&bytes, SUPPORTED), Err(IndexError::TooLarge));
    Ok(())
}

#[test]
fn an_index_of_exactly_1_mib_is_not_too_large() -> TestResult {
    // 末尾の空白で 1 MiB ちょうどにする（JSON として読めれば通る）
    let mut bytes = index_of(&[item("example.pdf-preview")]);
    bytes.resize(INDEX_MAX_BYTES, b' ');
    parse_index(&bytes, SUPPORTED)?;
    Ok(())
}

#[test]
fn a_schema_other_than_1_is_rejected() -> TestResult {
    let root = json!({ "schema": 2, "extensions": [item("example.pdf-preview")] });
    let bytes = serde_json::to_vec(&root)?;
    assert_eq!(
        parse_index(&bytes, SUPPORTED),
        Err(IndexError::UnsupportedSchema)
    );
    Ok(())
}

#[test]
fn broken_json_and_a_missing_extensions_list_are_malformed() -> TestResult {
    assert_eq!(
        parse_index(b"{\"schema\": 1, \"extensions\": [", SUPPORTED),
        Err(IndexError::Malformed)
    );
    assert_eq!(
        parse_index(b"{\"schema\": 1}", SUPPORTED),
        Err(IndexError::Malformed)
    );
    Ok(())
}

#[test]
fn more_than_500_items_are_rejected() -> TestResult {
    let items: Vec<Value> = (0..501).map(|i| item(&format!("ext.n{i}"))).collect();
    assert_eq!(
        parse_index(&index_of(&items), SUPPORTED),
        Err(IndexError::TooManyExtensions)
    );
    let items: Vec<Value> = (0..500).map(|i| item(&format!("ext.n{i}"))).collect();
    assert_eq!(
        parse_index(&index_of(&items), SUPPORTED)?.extensions.len(),
        500
    );
    Ok(())
}

#[test]
fn an_item_with_an_invalid_id_is_rejected() -> TestResult {
    for id in ["Bad", "a..b", "a-", "a b"] {
        assert_eq!(
            parse_index(&index_of(&[item(id)]), SUPPORTED),
            Err(IndexError::InvalidItem {
                index: 0,
                field: "id"
            }),
            "id {id:?} should be rejected"
        );
    }
    Ok(())
}

#[test]
fn a_download_url_that_is_not_https_is_rejected() -> TestResult {
    let mut bad = item("example.pdf-preview");
    bad["download"]["url"] = json!("http://example.com/pdf-preview.zip");
    assert_eq!(
        parse_index(&index_of(&[bad]), SUPPORTED),
        Err(IndexError::InvalidItem {
            index: 0,
            field: "download.url"
        })
    );
    Ok(())
}

#[test]
fn a_sha256_that_is_not_lowercase_hex_64_is_rejected() -> TestResult {
    let bad_values = [
        "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
    ];
    for value in bad_values {
        let mut bad = item("example.pdf-preview");
        bad["download"]["sha256"] = json!(value);
        assert_eq!(
            parse_index(&index_of(&[bad]), SUPPORTED),
            Err(IndexError::InvalidItem {
                index: 0,
                field: "download.sha256"
            })
        );
    }
    Ok(())
}

#[test]
fn a_size_over_32_mib_is_rejected() -> TestResult {
    let mut bad = item("example.pdf-preview");
    bad["download"]["size"] = json!(INDEX_MAX_DOWNLOAD_BYTES + 1);
    assert_eq!(
        parse_index(&index_of(&[bad]), SUPPORTED),
        Err(IndexError::InvalidItem {
            index: 0,
            field: "download.size"
        })
    );
    Ok(())
}

#[test]
fn an_item_with_permissions_is_excluded_and_counted() -> TestResult {
    let mut with_permission = item("example.with-permission");
    with_permission["permissions"] = json!(["network"]);
    let items = [
        item("example.pdf-preview"),
        with_permission,
        item("example.other"),
    ];
    let index = parse_index(&index_of(&items), SUPPORTED)?;
    assert_eq!(index.excluded_permissions, 1);
    let ids: Vec<&str> = index.extensions.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["example.pdf-preview", "example.other"]);
    Ok(())
}

#[test]
fn an_item_with_an_unsupported_api_version_is_excluded_and_counted() -> TestResult {
    let mut future = item("example.future");
    future["apiVersion"] = json!("2");
    let index = parse_index(&index_of(&[future, item("example.pdf-preview")]), SUPPORTED)?;
    assert_eq!(index.excluded_api_version, 1);
    assert_eq!(index.extensions.len(), 1);
    assert_eq!(index.extensions[0].id, "example.pdf-preview");
    Ok(())
}

#[test]
fn an_item_missing_a_required_key_is_an_item_shape_error() -> TestResult {
    let mut bad = item("example.pdf-preview");
    bad.as_object_mut()
        .expect("項目は JSON のオブジェクト")
        .remove("summary");
    assert_eq!(
        parse_index(&index_of(&[bad]), SUPPORTED),
        Err(IndexError::ItemShape { index: 0 })
    );
    Ok(())
}

#[test]
fn a_duplicate_id_is_rejected() -> TestResult {
    let items = [item("example.pdf-preview"), item("example.pdf-preview")];
    assert_eq!(
        parse_index(&index_of(&items), SUPPORTED),
        Err(IndexError::DuplicateId { index: 1 })
    );
    Ok(())
}

#[test]
fn unknown_keys_are_ignored() -> TestResult {
    let mut extra = item("example.pdf-preview");
    extra["future"] = json!({ "anything": true });
    let root = json!({ "schema": 1, "extensions": [extra], "generatedBy": "tool" });
    let bytes = serde_json::to_vec(&root)?;
    assert_eq!(parse_index(&bytes, SUPPORTED)?.extensions.len(), 1);
    Ok(())
}

#[test]
fn a_matching_download_verifies() -> TestResult {
    let bytes = b"zip bytes for the test";
    let sha: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    verify_download(bytes, &sha, bytes.len() as u64)?;
    Ok(())
}

#[test]
fn a_download_of_another_size_is_rejected() -> TestResult {
    let bytes = b"zip bytes for the test";
    let sha: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        verify_download(bytes, &sha, bytes.len() as u64 + 1),
        Err(IndexError::SizeMismatch)
    );
    Ok(())
}

#[test]
fn a_download_with_another_hash_is_rejected() -> TestResult {
    let bytes = b"zip bytes for the test";
    let other = b"zip bytes for the tesT";
    let sha: String = Sha256::digest(other)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        verify_download(bytes, &sha, bytes.len() as u64),
        Err(IndexError::HashMismatch)
    );
    Ok(())
}
