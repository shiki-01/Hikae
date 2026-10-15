// 実際の GitHub に対する動作確認（手動で実行する。自動テストでは動かさない）。
//
//   GITHUB_TOKEN=<個人用のアクセストークン（repo の権限）> cargo run -p core-remote --example live_smoke -- <owner>/<name>
//
// **確認用の空きのリポジトリ（捨ててよいもの）を指定すること**。既定ブランチに、`hikae-live-smoke/` フォルダの
// ファイルを追加するコミットを 3 つ作る（このクレートは削除をしないため、後片付けは手で行う）。
// 確認すること: 一覧・閲覧、追加（新規・置き換え・両方残す・追加しない）、バイト列の一致、過去の版、同名の扱い。

use core_github::{AccessToken, UserClient};
use core_remote::{OnExists, RemoteClient, UploadItem, UploadRequest};

fn check(ok: bool, what: &str) {
    println!("{} {what}", if ok { "OK  " } else { "FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

fn item(name: &str, content: &[u8], on_exists: OnExists) -> UploadItem {
    UploadItem {
        target_dir: "hikae-live-smoke".to_string(),
        name: name.to_string(),
        content: content.to_vec(),
        on_exists,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let repo_arg = std::env::args().nth(1).unwrap_or_default();
    let Some((owner, name)) = repo_arg.split_once('/') else {
        eprintln!("使い方: live_smoke <owner>/<name>（環境変数 GITHUB_TOKEN が必要）");
        std::process::exit(2);
    };
    let Ok(token) = std::env::var("GITHUB_TOKEN") else {
        eprintln!("環境変数 GITHUB_TOKEN がありません");
        std::process::exit(2);
    };
    let token = AccessToken::new(token);
    let client = RemoteClient::new();

    let user = UserClient::new()
        .fetch_user(&token)
        .await
        .expect("ユーザー情報");
    println!("ログイン中のユーザー: {}", user.login);

    let repos = client.list_repos(&token).await.expect("保存先の一覧");
    check(
        repos.iter().any(|r| r.owner == owner && r.name == name),
        "保存先の一覧に指定のリポジトリがある",
    );

    let summary = client
        .repo_summary(&token, owner, name)
        .await
        .expect("リポジトリ");
    println!("既定ブランチ: {}", summary.default_branch);
    let root = client
        .list_dir(&token, owner, name, "", None)
        .await
        .expect("ルートの一覧");
    println!("ルートの項目数: {}", root.entries.len());

    // 1. 新規追加（日本語名・バイナリ）
    let stamp = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let file = format!("確認_{stamp}.bin");
    let bytes: Vec<u8> = (0..=255u8).collect();
    let plan = client
        .plan_upload(
            &token,
            owner,
            name,
            &[core_remote::PlanItem {
                target_dir: "hikae-live-smoke".into(),
                name: file.clone(),
                size: bytes.len() as u64,
            }],
        )
        .await
        .expect("計画");
    check(!plan.items[0].exists, "追加前は同名が無い");
    let r1 = client
        .upload(
            &token,
            &user,
            owner,
            name,
            UploadRequest {
                message: Some("Hikae live smoke: 新規".into()),
                items: vec![item(&file, &bytes, OnExists::Skip)],
            },
        )
        .await
        .expect("追加");
    check(
        !r1.commit_sha.is_empty() && r1.saved.len() == 1,
        "新規のファイルが 1 件保存された",
    );

    let listing = client
        .list_dir(&token, owner, name, "hikae-live-smoke", None)
        .await
        .expect("一覧");
    let entry = listing.entries.iter().find(|e| e.name == file);
    check(entry.is_some(), "追加したファイルが一覧に出る");
    let back = client
        .fetch_blob(
            &token,
            owner,
            name,
            &entry.map(|e| e.sha.clone()).unwrap_or_default(),
            None,
        )
        .await
        .expect("取得");
    check(
        back == bytes,
        "取得したバイト列が追加したものと完全に一致する（0〜255 の全バイト）",
    );

    // 2. 同名: 置き換える → 両方残す → 追加しない
    let replaced = client
        .upload(
            &token,
            &user,
            owner,
            name,
            UploadRequest {
                message: Some("Hikae live smoke: 置き換え".into()),
                items: vec![item(&file, b"v2", OnExists::Replace)],
            },
        )
        .await
        .expect("置き換え");
    check(
        replaced.saved == vec![format!("hikae-live-smoke/{file}")],
        "置き換えで同じパスに保存された",
    );
    let history = client
        .file_history(&token, owner, name, &format!("hikae-live-smoke/{file}"))
        .await
        .expect("履歴");
    check(history.len() >= 2, "過去の版が履歴に残っている（2 件以上）");
    let (old_blob, _) = client
        .file_at(
            &token,
            owner,
            name,
            &format!("hikae-live-smoke/{file}"),
            &history[1].sha,
        )
        .await
        .expect("過去の版");
    let old = client
        .fetch_blob(&token, owner, name, &old_blob, None)
        .await
        .expect("過去の版の内容");
    check(
        old == bytes,
        "過去の版の内容が、置き換える前のものと一致する",
    );

    let both = client
        .upload(
            &token,
            &user,
            owner,
            name,
            UploadRequest {
                message: Some("Hikae live smoke: 両方残す".into()),
                items: vec![item(&file, b"v3", OnExists::KeepBoth)],
            },
        )
        .await
        .expect("両方残す");
    check(both.renamed.len() == 1, "両方残すで名前が変わった");
    let skipped = client
        .upload(
            &token,
            &user,
            owner,
            name,
            UploadRequest {
                message: None,
                items: vec![item(&file, b"v4", OnExists::Skip)],
            },
        )
        .await
        .expect("追加しない");
    check(
        skipped.commit_sha.is_empty() && skipped.skipped.len() == 1,
        "追加しないでは、コミットが作られない",
    );

    println!("\nすべて確認できました。確認用のファイルは hikae-live-smoke/ に残っています（手で削除してください）。");
}
