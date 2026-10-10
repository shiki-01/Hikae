// テスト専用の偽 llama-server（実機の代わり）。サイドカー制御の起動・health・終了・再起動を確認するために使う。
// 利用者向けの成果物ではない。
//
// 引数: `-m <path> --host <host> --port <port>`（他の引数は無視）。`--host` が 127.0.0.1 以外なら終了コード 2 で失敗する。
// 起動のたびに、モデルと同じフォルダの `starts.log` に 1 行追記する（起動回数の確認用）。
// モデルのファイル名で挙動を変える（テストが選ぶ）:
// - `no-health` を含む: `/health` が 503 を返す（起動待ちのタイムアウト用）
// - `exit-on-chat` を含む: `/v1/chat/completions` を受けたら応答せずに終了する（異常終了の再現用）
//
// 応答はすべて `Connection: close` で返す。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

/// テストが選ぶ挙動
#[derive(Clone, Copy)]
struct Mode {
    no_health: bool,
    exit_on_chat: bool,
}

const CHAT_JSON: &str = r#"{"choices":[{"message":{"role":"assistant","content":"保存メモです。"},"finish_reason":"stop"}]}"#;

fn main() {
    if let Err(code) = run() {
        std::process::exit(code);
    }
}

fn run() -> Result<(), i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut model: Option<PathBuf> = None;
    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-m" => {
                model = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--host" => {
                host = args.get(i + 1).cloned();
                i += 2;
            }
            "--port" => {
                port = args.get(i + 1).and_then(|p| p.parse::<u16>().ok());
                i += 2;
            }
            _ => i += 1,
        }
    }
    let model = model.ok_or(2)?;
    if host.as_deref() != Some("127.0.0.1") {
        return Err(2);
    }
    let port = port.ok_or(2)?;
    if !model.is_file() {
        return Err(3);
    }
    let name = model
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mode = Mode {
        no_health: name.contains("no-health"),
        exit_on_chat: name.contains("exit-on-chat"),
    };
    record_start(&model);

    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| 4)?;
    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || handle(stream, mode));
    }
    Ok(())
}

/// 起動回数を記録する（失敗しても起動は続ける）
fn record_start(model: &std::path::Path) {
    if let Some(dir) = model.parent() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("starts.log"))
        {
            let _ = writeln!(file, "start {}", std::process::id());
        }
    }
}

fn handle(mut stream: TcpStream, mode: Mode) {
    let Some((method, path)) = read_request(&mut stream) else {
        return;
    };
    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/health") => {
            if mode.no_health {
                http_response(503, r#"{"error":{"message":"Loading model"}}"#)
            } else {
                http_response(200, r#"{"status":"ok"}"#)
            }
        }
        ("POST", "/v1/chat/completions") => {
            if mode.exit_on_chat {
                std::process::exit(1);
            }
            http_response(200, CHAT_JSON)
        }
        ("GET", "/api/tags") => http_response(200, r#"{"models":[{"name":"fake-model"}]}"#),
        _ => http_response(404, r#"{"error":"not found"}"#),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// 要求の行とヘッダ、本文を読み、メソッドと経路を返す。本文は読み捨てる。
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 || buf.len() > 64 * 1024 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let content_length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body_read = buf.len() - header_end;
    while body_read < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body_read += n;
    }
    Some((method, path))
}

fn http_response(code: u16, body: &str) -> String {
    let reason = match code {
        200 => "OK",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
