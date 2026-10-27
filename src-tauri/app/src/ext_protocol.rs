// `ext` カスタムプロトコル（設計書 11 章の表示拡張）。登録済みの拡張のフォルダ内のファイルを、iframe に読ませる。
//
// URL 形式: `ext://localhost/<拡張 ID>/<URL エンコードした相対パス>`
// - Windows では WebView が `http://ext.localhost/...` に変換するため、両方のホストを受け付ける
// - 配信するのは登録済みの拡張のフォルダの中の、許可リストの拡張子の通常のファイルだけ（解決は core-preview の `resolve_asset`）
// - 読み取りのみ。作業フォルダ・リポジトリ・設定には触れない
// - 応答には、nosniff・CORS（opaque origin の iframe から module script を読むため）・CSP を付ける
// - 失敗は本文の無い 4xx 等だけを返す。パス・内容はログにも応答にも出さない

use crate::AppState;
use core_preview::{
    asset_mime_of, decode_asset_path, resolve_asset, AssetError, ExtensionHost, ASSET_MAX_BYTES,
};
use tauri::http::{Method, Request, Response, StatusCode, Uri};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

/// プロトコル名（`register_asynchronous_uri_scheme_protocol` に渡す）
pub const SCHEME: &str = "ext";

/// すべての応答に付ける CSP（設計書 11.1）。拡張の画面はネットワークに出られず、フレームも作れない
///
/// macOS の WebKit は、sandbox の iframe（opaque origin）では `'self'` を `ext://localhost` に一致させない。
/// このため `ext:` のスキームを明示する（Windows は `http://ext.localhost` で `'self'` が一致する）
const CSP: &str = "default-src 'none'; script-src 'self' ext: blob:; \
style-src 'self' ext: 'unsafe-inline'; img-src 'self' ext: data: blob:; font-src 'self' ext:; \
connect-src 'none'; frame-src 'none'; worker-src blob:; base-uri 'none'; form-action 'none'";

/// プロトコルの要求を処理する。読み込みは別スレッドで行い、UI スレッドを塞がない
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let extensions = ctx.app_handle().state::<AppState>().extensions.clone();
    let method = request.method().clone();
    let target = parse_target(request.uri());

    tauri::async_runtime::spawn_blocking(move || {
        let response = build_response(&extensions, &method, target);
        responder.respond(response);
    });
}

/// URL から取り出した要求の中身（パスは未デコード）
#[derive(Debug, PartialEq, Eq)]
struct Target {
    extension_id: String,
    /// URL のエンコードのままの相対パス
    encoded_path: String,
}

/// URL を分解する。ホストは `localhost`（macOS・Linux）と `ext.localhost`（Windows）を受け付ける
fn parse_target(uri: &Uri) -> Option<Target> {
    match uri.host() {
        Some("localhost") | Some("ext.localhost") => {}
        _ => return None,
    }
    let rest = uri.path().strip_prefix('/')?;
    let (extension_id, encoded_path) = rest.split_once('/')?;
    if extension_id.is_empty() || encoded_path.is_empty() {
        return None;
    }
    Some(Target {
        extension_id: extension_id.to_string(),
        encoded_path: encoded_path.to_string(),
    })
}

/// 応答を組み立てる（同期処理。呼び出し側が別スレッドで実行する）
fn build_response(
    extensions: &ExtensionHost,
    method: &Method,
    target: Option<Target>,
) -> Response<Vec<u8>> {
    if *method != Method::GET {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(target) = target else {
        return empty(StatusCode::BAD_REQUEST);
    };
    // 登録されていない拡張は、存在しないものと同じ扱い
    let Some(root) = extensions.extension_root(&target.extension_id) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let rel = match decode_asset_path(&target.encoded_path) {
        Ok(rel) => rel,
        Err(err) => return empty(status_for(err)),
    };
    // 許可リストに無い種類（wasm を含む）は、場所を調べる前に拒否する
    let Some(mime) = asset_mime_of(&rel) else {
        return empty(StatusCode::FORBIDDEN);
    };
    let path = match resolve_asset(&root, &rel) {
        Ok(path) => path,
        Err(err) => return empty(status_for(err)),
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return empty(StatusCode::NOT_FOUND);
    };
    if metadata.len() > ASSET_MAX_BYTES {
        return empty(StatusCode::PAYLOAD_TOO_LARGE);
    }
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() as u64 <= ASSET_MAX_BYTES => asset_response(mime, bytes),
        Ok(_) => empty(StatusCode::PAYLOAD_TOO_LARGE),
        Err(_) => empty(StatusCode::NOT_FOUND),
    }
}

/// 配信の失敗を HTTP の状態へ写す（種類だけを使い、パスは使わない）
fn status_for(err: AssetError) -> StatusCode {
    match err {
        AssetError::Rejected | AssetError::Outside | AssetError::NotAllowed => {
            StatusCode::FORBIDDEN
        }
        AssetError::NotFound => StatusCode::NOT_FOUND,
        AssetError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
    }
}

/// ファイルの応答（本文あり）
fn asset_response(mime: &'static str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    with_common_headers(StatusCode::OK, mime)
        .body(bytes)
        .unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
}

/// 本文のない応答（エラー）
fn empty(status: StatusCode) -> Response<Vec<u8>> {
    with_common_headers(status, "text/plain; charset=utf-8")
        .body(Vec::new())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn with_common_headers(
    status: StatusCode,
    content_type: &'static str,
) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("X-Content-Type-Options", "nosniff")
        // opaque origin（sandbox の iframe）の要求で module script を読むため
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-cache")
        .header("Content-Security-Policy", CSP)
}
