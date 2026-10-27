// credential helper が実際の git 実行で使われることの確認。
// 認証を要求するだけのローカル HTTP サーバーを立て、helper 経由で認証情報が送られるかを見る。
// （外部ネットワークには接続しない。helper が返す認証情報はテスト用の固定値）

use core_git::GitRunner;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 認証ヘッダ付きのリクエストを受けたかを記録する、認証必須のサーバー。
/// 認証なしには 401 を、テスト用の認証情報（u:p）が付いていれば 404 を返す。
fn start_auth_server() -> Result<(u16, Arc<AtomicBool>), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let saw_credentials = Arc::new(AtomicBool::new(false));
    let flag = saw_credentials.clone();
    std::thread::spawn(move || {
        // 1 回のテストで来る接続は数回。来なくなったら終了する
        let _ = listener.set_nonblocking(false);
        for stream in listener.incoming().take(8) {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            // base64("u:p") = dTpw
            let authed = request.contains("authorization: basic dtpw");
            let response = if authed {
                flag.store(true, Ordering::SeqCst);
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            } else {
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok((port, saw_credentials))
}

const HELPER: &str = "!f() { echo username=u; echo password=p; }; f";

#[test]
fn credential_helper_is_used_for_network_commands() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (port, saw_credentials) = start_auth_server()?;
    let url = format!("http://127.0.0.1:{port}/repo.git");

    let runner = GitRunner::from_path_env().with_credential_helper(HELPER);
    let out = runner.run(tmp.path(), &["ls-remote", &url])?;
    // サーバーは認証後に 404 を返すので git 自体は失敗するが、認証情報は送られている
    assert_ne!(out.code, 0);
    assert!(
        saw_credentials.load(Ordering::SeqCst),
        "helper の認証情報が送られていません: {}",
        out.stderr
    );
    Ok(())
}

#[test]
fn without_a_helper_no_credentials_are_sent() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let (port, saw_credentials) = start_auth_server()?;
    let url = format!("http://127.0.0.1:{port}/repo.git");

    let out = GitRunner::from_path_env().run(tmp.path(), &["ls-remote", &url])?;
    assert_ne!(out.code, 0);
    assert!(!saw_credentials.load(Ordering::SeqCst));
    // 端末での入力待ちにならず、認証エラーとして終了する
    assert!(out
        .stderr
        .to_ascii_lowercase()
        .contains("terminal prompts disabled"));
    Ok(())
}

#[test]
fn credential_helper_is_not_added_to_local_commands() -> Result<(), Box<dyn std::error::Error>> {
    // ネットワークを使わないコマンドは、helper を設定しても通常どおり動く
    let tmp = tempfile::tempdir()?;
    let runner = GitRunner::from_path_env().with_credential_helper(HELPER);
    runner.run_ok(tmp.path(), &["init", "-b", "main"])?;
    let out = runner.run_ok(tmp.path(), &["status", "--porcelain=v2", "-z"])?;
    assert!(out.stdout.is_empty());
    Ok(())
}

#[test]
fn github_ssh_urls_are_read_as_https() -> Result<(), Box<dyn std::error::Error>> {
    // 通信に付ける URL の読み替え（設計書 6.1）が git に効くことを、通信せずに確かめる。
    // `remote get-url` は insteadOf を適用した URL を返す。リポジトリの設定は SSH 形式のまま
    let tmp = tempfile::tempdir()?;
    let runner = GitRunner::from_path_env();
    runner.run_ok(tmp.path(), &["init", "-b", "main"])?;
    runner.run_ok(
        tmp.path(),
        &["remote", "add", "scp", "git@github.com:owner/repo.git"],
    )?;
    runner.run_ok(
        tmp.path(),
        &[
            "remote",
            "add",
            "ssh",
            "ssh://git@github.com/owner/repo.git",
        ],
    )?;
    runner.run_ok(
        tmp.path(),
        &["remote", "add", "other", "git@example.com:owner/repo.git"],
    )?;
    let rewrite = [
        "-c",
        "url.https://github.com/.insteadOf=git@github.com:",
        "-c",
        "url.https://github.com/.insteadOf=ssh://git@github.com/",
    ];
    let get_url = |name: &str| -> Result<String, Box<dyn std::error::Error>> {
        let mut args: Vec<&str> = rewrite.to_vec();
        args.extend(["remote", "get-url", name]);
        Ok(String::from_utf8(runner.run_ok(tmp.path(), &args)?.stdout)?
            .trim()
            .to_string())
    };
    assert_eq!(get_url("scp")?, "https://github.com/owner/repo.git");
    assert_eq!(get_url("ssh")?, "https://github.com/owner/repo.git");
    // GitHub 以外はそのまま
    assert_eq!(get_url("other")?, "git@example.com:owner/repo.git");
    // 読み替えなしでは、設定どおりの SSH 形式
    let out = runner.run_ok(tmp.path(), &["remote", "get-url", "scp"])?;
    assert_eq!(
        String::from_utf8(out.stdout)?.trim(),
        "git@github.com:owner/repo.git"
    );
    Ok(())
}
