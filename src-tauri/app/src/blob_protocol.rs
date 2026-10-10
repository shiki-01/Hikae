// `app-blob` カスタムプロトコル（設計書 9.3）。画像比較（S4）が、過去の版と作業フォルダの画像を
// `<img>` と canvas に読ませるための経路。読み取りのみで、状態は変えない。
//
// URL 形式: `app-blob://localhost/<プロジェクト ID>/<rev>/<URL エンコードしたパス>`
// - Windows では WebView が `http://app-blob.localhost/...` に変換するため、両方の形を受け付ける
// - `rev` は画面の時点 ID（`work`・`current`・`this`・`cloud`・16 進の短縮 ID）。解決は `version` に集約している
// - 応答には内容の mime・`nosniff`・canvas で画素を読むための CORS・`no-store` を付ける
// - 失敗は 4xx / 5xx と空のボディだけを返す。パスやトークンはログにも応答にも出さない

use crate::{version, AppState};
use core_ops::{Blob, Ops, OpsError};
use tauri::http::{Method, Request, Response, StatusCode, Uri};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

/// プロトコル名（`register_asynchronous_uri_scheme_protocol` に渡す）
pub const SCHEME: &str = "app-blob";

/// 読み取る上限（設計書 9.3）。これを超える画像は読まずに拒否する
const MAX_BYTES: u64 = 50 * 1024 * 1024;

/// プロトコルの要求を処理する。重い読み取りは別スレッドで行い、UI スレッドを塞がない
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let store = ctx.app_handle().state::<AppState>().store_clone();
    let method = request.method().clone();
    let target = parse_target(request.uri());

    tauri::async_runtime::spawn_blocking(move || {
        let response = build_response(&store, &method, target);
        responder.respond(response);
    });
}

/// URL から取り出した要求の中身
#[derive(Debug, PartialEq, Eq)]
struct Target {
    project_id: String,
    rev: String,
    relative_path: String,
}

/// URL を分解する。ホストは `localhost`（macOS・Linux）と `app-blob.localhost`（Windows）を受け付ける。
/// 形が合わなければ None
fn parse_target(uri: &Uri) -> Option<Target> {
    match uri.host() {
        Some("localhost") | Some("app-blob.localhost") => {}
        _ => return None,
    }
    let rest = uri.path().strip_prefix('/')?;
    let mut parts = rest.splitn(3, '/');
    let project_id = parts.next().filter(|s| !s.is_empty())?;
    let rev = parts.next().filter(|s| !s.is_empty())?;
    let encoded = parts.next().filter(|s| !s.is_empty())?;
    let relative_path = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .ok()?
        .into_owned();
    Some(Target {
        project_id: project_id.to_string(),
        rev: rev.to_string(),
        relative_path,
    })
}

/// 応答を組み立てる（同期処理。呼び出し側が別スレッドで実行する）
fn build_response(
    store: &std::sync::Arc<std::sync::Mutex<core_store::Store>>,
    method: &Method,
    target: Option<Target>,
) -> Response<Vec<u8>> {
    if *method != Method::GET && *method != Method::HEAD {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(target) = target else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let Some(project_path) = project_path(store, &target.project_id) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let ops = Ops::new(crate::git_runner());

    let rev = match version::resolve_rev(&ops, &project_path, &target.rev) {
        Ok(Some(rev)) => rev,
        // 存在しない版は「ファイルなし」と同じ扱い。形の不正な値は要求の誤り
        Ok(None) => return empty(StatusCode::NOT_FOUND),
        Err(OpsError::InvalidInput(_)) => return empty(StatusCode::BAD_REQUEST),
        Err(_) => return empty(StatusCode::NOT_FOUND),
    };

    match ops.read_blob(&project_path, &rev, &target.relative_path, MAX_BYTES) {
        Ok(blob) => blob_response(blob),
        Err(err) => empty(status_for(&err)),
    }
}

/// プロジェクト ID からパスを引く。未登録・ロック失敗は None
fn project_path(
    store: &std::sync::Arc<std::sync::Mutex<core_store::Store>>,
    project_id: &str,
) -> Option<std::path::PathBuf> {
    let guard = store.lock().ok()?;
    guard.get_project(project_id).ok().map(|p| p.path)
}

/// 読み取りの失敗を HTTP の状態へ写す。見つからない → 404、要求が不正 → 400、そのほか → 500
fn status_for(err: &OpsError) -> StatusCode {
    match err {
        OpsError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        OpsError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// 画像の応答（本文あり）
fn blob_response(blob: Blob) -> Response<Vec<u8>> {
    with_common_headers(StatusCode::OK, blob.mime)
        .body(blob.bytes)
        .unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
}

/// 本文のない応答（エラー）。CORS の付いた応答として返し、画面が状態を読めるようにする
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
        // canvas で画素を読むため（`<img crossorigin="anonymous">` の前提）
        .header("Access-Control-Allow-Origin", "*")
        // 作業フォルダの内容は変わるため、キャッシュしない
        .header("Cache-Control", "no-store")
}
