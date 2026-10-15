// アップロード（追加して保存）（設計書 14.5）。
//
// 手順は GitHub の Git Data API で行う（blob → tree → commit → fast-forward の ref 更新）。
// ref の更新は `force: false` の 1 回だけで、422（他の端末が先に更新した）のときは
// 最初から作り直す（blob は再利用する）。3 回とも 422 なら何も変えずに終わる。

use crate::allow::{update_ref_body, Call};
use crate::client::{parse, RemoteClient};
use crate::encode::is_object_id;
use crate::error::RemoteError;
use crate::model::{
    OnExists, PlanItem, PlannedItem, Renamed, UploadItem, UploadPlan, UploadRequest, UploadResult,
    MAX_FILE_BYTES, MAX_ITEMS, MAX_TOTAL_BYTES,
};
use crate::path::{check_dir, check_name, check_repo, check_target_path};
use crate::read::Folder;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use core_github::{AccessToken, User};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

/// 422 のときに作り直す最大回数（最初の 1 回を含む）
const MAX_ATTEMPTS: u32 = 3;
/// 保存メッセージの長さの上限（文字数）
const MESSAGE_MAX_CHARS: usize = 500;
/// 通常ファイルのモード（実行ビットは見えないので固定）
const FILE_MODE: &str = "100644";

/// `POST` の応答のうち、作られたオブジェクトの ID
#[derive(Deserialize)]
struct ShaResponse {
    sha: String,
}

/// 保存先のフォルダにある既存の項目（同名の確認に使う）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExistingEntry {
    pub name: String,
    /// フォルダかどうか（サブモジュール・シンボリックリンクは通常のファイルとして扱う）
    pub is_dir: bool,
}

/// 検査済みの追加の 1 件（フォルダと名前は NFC に正規化済み）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub dir: String,
    pub name: String,
    pub on_exists: OnExists,
}

/// 同名の解決の結果（要求の項目と同じ順）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// 追加しない
    Skip { path: String },
    /// 保存する。`renamed` は `両方残す` で名前を変えたとき
    Write { path: String, renamed: bool },
}

/// 件数と大きさの検査（通信より前）
fn check_sizes(count: usize, sizes: impl Iterator<Item = u64>) -> Result<(), RemoteError> {
    if count == 0 {
        return Err(RemoteError::EmptyRequest);
    }
    if count > MAX_ITEMS {
        return Err(RemoteError::TooMany);
    }
    let mut total: u64 = 0;
    for size in sizes {
        if size > MAX_FILE_BYTES {
            return Err(RemoteError::TooLarge);
        }
        total = total.saturating_add(size);
    }
    if total > MAX_TOTAL_BYTES {
        return Err(RemoteError::TooLarge);
    }
    Ok(())
}

/// 項目の名前とフォルダを検査し、正規化した値にする。
fn prepare(items: &[UploadItem]) -> Result<Vec<Prepared>, RemoteError> {
    items
        .iter()
        .map(|item| {
            let dir = check_dir(&item.target_dir)?;
            let name = check_name(&item.name)?;
            check_target_path(&dir, &name)?;
            Ok(Prepared {
                dir,
                name,
                on_exists: item.on_exists,
            })
        })
        .collect()
}

/// 同名の比較に使うキー。GitHub 側の名前が NFD で保存されていても同じ名前とみなすため NFC にし、
/// さらに大文字小文字を区別しない（比較専用。保存する名前には使わない）。
fn key(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// フォルダとファイル名からリポジトリのルートからのパスを作る（検査済みの値のみ）。
fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// `名前 (n).拡張子` の形の名前を作る。最後の `.` より前を名前、後ろを拡張子とみなす。
fn numbered(name: &str, n: u64) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({}){}", &name[..i], n, &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// フォルダの中の同名の項目を探す（大文字小文字を区別しない）。同名がフォルダならエラー。
fn lookup(
    existing: &HashMap<String, Vec<ExistingEntry>>,
    dir: &str,
    name: &str,
) -> Result<Option<ExistingEntry>, RemoteError> {
    let wanted = key(name);
    let found = existing
        .get(dir)
        .and_then(|entries| entries.iter().find(|e| key(&e.name) == wanted))
        .cloned();
    match found {
        Some(entry) if entry.is_dir => Err(RemoteError::NameIsFolder(join(dir, name))),
        other => Ok(other),
    }
}

/// 同名の解決（設計書 14.5 の手順 3）。純関数。
///
/// - 同名が無いものは、そのまま追加する
/// - 同名があるものは、項目ごとの `on_exists` で扱う。`両方残す` は既存と他の項目と重ならない名前にする
/// - 最終的な保存先が（大文字小文字を区別せず）重なるものは `DuplicateTarget`
pub(crate) fn resolve(
    items: &[Prepared],
    existing: &HashMap<String, Vec<ExistingEntry>>,
) -> Result<Vec<Decision>, RemoteError> {
    let founds: Vec<Option<ExistingEntry>> = items
        .iter()
        .map(|item| lookup(existing, &item.dir, &item.name))
        .collect::<Result<_, _>>()?;

    // 使われている名前。既存の項目と、これから新しく作る項目の名前
    let mut taken: HashSet<(String, String)> = HashSet::new();
    for (dir, entries) in existing {
        for entry in entries {
            taken.insert((dir.clone(), key(&entry.name)));
        }
    }
    for (item, found) in items.iter().zip(&founds) {
        if found.is_none() {
            taken.insert((item.dir.clone(), key(&item.name)));
        }
    }

    let mut decisions = Vec::with_capacity(items.len());
    for (item, found) in items.iter().zip(founds) {
        let decision = match found {
            None => Decision::Write {
                path: join(&item.dir, &item.name),
                renamed: false,
            },
            Some(entry) => match item.on_exists {
                OnExists::Skip => Decision::Skip {
                    path: join(&item.dir, &entry.name),
                },
                OnExists::Replace => Decision::Write {
                    path: join(&item.dir, &entry.name),
                    renamed: false,
                },
                OnExists::KeepBoth => {
                    let mut n: u64 = 2;
                    let candidate = loop {
                        let candidate = numbered(&item.name, n);
                        if !taken.contains(&(item.dir.clone(), key(&candidate))) {
                            break candidate;
                        }
                        n += 1;
                    };
                    taken.insert((item.dir.clone(), key(&candidate)));
                    Decision::Write {
                        path: join(&item.dir, &candidate),
                        renamed: true,
                    }
                }
            },
        };
        decisions.push(decision);
    }

    // 保存する先が重ならないこと（大文字小文字を区別しない）
    let mut seen: HashSet<String> = HashSet::new();
    for decision in &decisions {
        if let Decision::Write { path, .. } = decision {
            if !seen.insert(key(path)) {
                return Err(RemoteError::DuplicateTarget(path.clone()));
            }
        }
    }
    Ok(decisions)
}

/// 保存メッセージを決める。空白のみ・None のときは既定の文言（件数入り）。
fn build_message(message: Option<&str>, count: usize) -> String {
    match message {
        Some(m) if !m.trim().is_empty() => m.chars().take(MESSAGE_MAX_CHARS).collect(),
        _ => format!("スマホから追加（{count} 件）"),
    }
}

impl RemoteClient {
    /// 追加する前の確認。同名のファイルがあるかを、項目ごとに返す（画面の確認ダイアログに使う）。
    /// 保存は行わない。
    pub async fn plan_upload(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        items: &[PlanItem],
    ) -> Result<UploadPlan, RemoteError> {
        check_repo(owner, name)?;
        check_sizes(items.len(), items.iter().map(|i| i.size))?;
        let prepared: Vec<Prepared> = items
            .iter()
            .map(|item| {
                let dir = check_dir(&item.target_dir)?;
                let name = check_name(&item.name)?;
                check_target_path(&dir, &name)?;
                Ok(Prepared {
                    dir,
                    name,
                    // 計画では扱いを決めない（保存しないので使わない）
                    on_exists: OnExists::Skip,
                })
            })
            .collect::<Result<_, RemoteError>>()?;

        let branch = self.repo_summary(token, owner, name).await?.default_branch;
        let head = self.branch_head(token, owner, name, &branch).await?;
        let dirs: Vec<String> = prepared.iter().map(|p| p.dir.clone()).collect();
        let existing = self
            .existing_names(token, owner, name, &head, &dirs)
            .await?;

        let mut planned = Vec::with_capacity(prepared.len());
        for item in &prepared {
            let exists = lookup(&existing, &item.dir, &item.name)?.is_some();
            planned.push(PlannedItem {
                target_path: join(&item.dir, &item.name),
                exists,
            });
        }
        Ok(UploadPlan {
            branch,
            items: planned,
        })
    }

    /// 追加して保存する（1 回のコミット）。`user` の noreply アドレスを作者と committer にする。
    pub async fn upload(
        &self,
        token: &AccessToken,
        user: &User,
        owner: &str,
        name: &str,
        req: UploadRequest,
    ) -> Result<UploadResult, RemoteError> {
        check_repo(owner, name)?;
        check_sizes(
            req.items.len(),
            req.items.iter().map(|i| i.content.len() as u64),
        )?;
        let prepared = prepare(&req.items)?;
        let dirs: Vec<String> = prepared.iter().map(|p| p.dir.clone()).collect();
        let email = user.noreply_email();

        // 試行をまたいで blob を再利用する（内容が同じなら ID も同じ）
        let mut blobs: Vec<Option<String>> = vec![None; req.items.len()];

        for attempt in 1..=MAX_ATTEMPTS {
            // 1. 既定ブランチの先頭と、そのツリー
            let branch = self.repo_summary(token, owner, name).await?.default_branch;
            let head = self.branch_head(token, owner, name, &branch).await?;
            let base_tree = self.commit_tree(token, owner, name, &head).await?;

            // 2. 同名の確認と、項目ごとの扱い
            let existing = self
                .existing_names(token, owner, name, &head, &dirs)
                .await?;
            let decisions = resolve(&prepared, &existing)?;

            let skipped: Vec<String> = decisions
                .iter()
                .filter_map(|d| match d {
                    Decision::Skip { path } => Some(path.clone()),
                    Decision::Write { .. } => None,
                })
                .collect();
            let saved: Vec<String> = decisions
                .iter()
                .filter_map(|d| match d {
                    Decision::Write { path, .. } => Some(path.clone()),
                    Decision::Skip { .. } => None,
                })
                .collect();
            if saved.is_empty() {
                // 全部 `追加しない` のときは、ここで終わる（通信はここまで）
                return Ok(UploadResult {
                    commit_sha: String::new(),
                    saved,
                    skipped,
                    renamed: Vec::new(),
                });
            }
            let renamed: Vec<Renamed> = decisions
                .iter()
                .zip(&prepared)
                .filter_map(|(d, p)| match d {
                    Decision::Write {
                        path,
                        renamed: true,
                    } => Some(Renamed {
                        from: join(&p.dir, &p.name),
                        to: path.clone(),
                    }),
                    _ => None,
                })
                .collect();

            // 3. blob（内容ごと。試行をまたいで再利用）
            for (i, decision) in decisions.iter().enumerate() {
                if !matches!(decision, Decision::Write { .. }) || blobs[i].is_some() {
                    continue;
                }
                let sha = self
                    .create_blob(token, owner, name, &req.items[i].content)
                    .await?;
                blobs[i] = Some(sha);
            }

            // 4. tree（base_tree の上に、保存する項目だけを載せる）
            let mut entries = Vec::with_capacity(saved.len());
            for (i, decision) in decisions.iter().enumerate() {
                if let Decision::Write { path, .. } = decision {
                    let sha = blobs[i].clone().ok_or(RemoteError::InvalidResponse)?;
                    entries.push(json!({
                        "path": path,
                        "mode": FILE_MODE,
                        "type": "blob",
                        "sha": sha,
                    }));
                }
            }
            let tree_value = self
                .send(
                    token,
                    Call::CreateTree { owner, name },
                    &[],
                    Some(json!({ "base_tree": base_tree, "tree": entries })),
                )
                .await?;
            let tree: ShaResponse = parse(tree_value)?;
            if !is_object_id(&tree.sha) {
                return Err(RemoteError::InvalidResponse);
            }

            // 5. commit（親は先頭の 1 つだけ。作者と committer は noreply アドレス）
            let identity = json!({ "name": user.login, "email": email });
            let commit_value = self
                .send(
                    token,
                    Call::CreateCommit { owner, name },
                    &[],
                    Some(json!({
                        "message": build_message(req.message.as_deref(), saved.len()),
                        "tree": tree.sha,
                        "parents": [head],
                        "author": identity,
                        "committer": identity,
                    })),
                )
                .await?;
            let commit: ShaResponse = parse(commit_value)?;
            if !is_object_id(&commit.sha) {
                return Err(RemoteError::InvalidResponse);
            }

            // 6. ref の更新（fast-forward のみ）
            let update = self
                .send(
                    token,
                    Call::UpdateRef {
                        owner,
                        name,
                        branch: &branch,
                    },
                    &[],
                    Some(update_ref_body(&commit.sha)),
                )
                .await;
            match update {
                Ok(_) => {
                    return Ok(UploadResult {
                        commit_sha: commit.sha,
                        saved,
                        skipped,
                        renamed,
                    })
                }
                Err(RemoteError::NotFastForward) if attempt < MAX_ATTEMPTS => continue,
                Err(e) => return Err(e),
            }
        }
        // 最後の試行は必ず上の match で return している
        Err(RemoteError::NotFastForward)
    }

    /// 保存先のフォルダごとの既存の項目。フォルダが無ければ空。
    /// フォルダがファイルのとき、1,000 件を超えて同名を確かめられないときは止める（上書きを防ぐため）。
    async fn existing_names(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        head: &str,
        dirs: &[String],
    ) -> Result<HashMap<String, Vec<ExistingEntry>>, RemoteError> {
        let mut map: HashMap<String, Vec<ExistingEntry>> = HashMap::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for dir in dirs {
            if !seen.insert(dir.as_str()) {
                continue;
            }
            let entries = match self.folder(token, owner, name, dir, head).await {
                Ok(Folder::Listed(listing)) => {
                    if listing.truncated {
                        return Err(RemoteError::TooMany);
                    }
                    listing
                        .entries
                        .into_iter()
                        .map(|e| ExistingEntry {
                            name: e.name,
                            is_dir: e.kind == "dir",
                        })
                        .collect()
                }
                Ok(Folder::File) => {
                    return Err(RemoteError::InvalidPath(
                        "保存先のフォルダが、ファイルになっています".to_string(),
                    ))
                }
                // フォルダが無いときは空として扱う（新しく作られる）。
                // ただし、途中の階層が既存のファイルのときは作れない（tree の作成が失敗する）ので、先に止める
                Err(RemoteError::NotFound) => {
                    self.check_ancestors_are_folders(token, owner, name, head, dir)
                        .await?;
                    Vec::new()
                }
                Err(e) => return Err(e),
            };
            map.insert(dir.clone(), entries);
        }
        Ok(map)
    }

    /// 存在しないフォルダ `dir` の祖先をたどり、最も近い既存の階層で、`dir` へ向かう次の名前が
    /// ファイル（フォルダ以外）になっていないかを確かめる。なっていれば `InvalidPath`。
    async fn check_ancestors_are_folders(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        head: &str,
        dir: &str,
    ) -> Result<(), RemoteError> {
        let segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        // 近い祖先（親）から根へ向かって調べる。`child` は、その祖先の直下で `dir` へ向かう名前
        for depth in (0..segments.len()).rev() {
            let ancestor = segments[..depth].join("/");
            let child = segments[depth];
            match self.folder(token, owner, name, &ancestor, head).await {
                Ok(Folder::Listed(listing)) => {
                    let wanted = key(child);
                    let blocked = listing
                        .entries
                        .iter()
                        .any(|e| key(&e.name) == wanted && e.kind != "dir");
                    return if blocked {
                        Err(RemoteError::InvalidPath(
                            "保存先の途中に、同じ名前のファイルがあります".to_string(),
                        ))
                    } else {
                        Ok(())
                    };
                }
                Ok(Folder::File) => {
                    return Err(RemoteError::InvalidPath(
                        "保存先の途中が、ファイルになっています".to_string(),
                    ))
                }
                // この祖先も無いときは、さらに上を調べる
                Err(RemoteError::NotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// blob を 1 つ作り、その ID を返す。
    async fn create_blob(
        &self,
        token: &AccessToken,
        owner: &str,
        name: &str,
        content: &[u8],
    ) -> Result<String, RemoteError> {
        let value = self
            .send(
                token,
                Call::CreateBlob { owner, name },
                &[],
                Some(json!({
                    "content": STANDARD.encode(content),
                    "encoding": "base64",
                })),
            )
            .await?;
        let blob: ShaResponse = parse(value)?;
        if !is_object_id(&blob.sha) {
            return Err(RemoteError::InvalidResponse);
        }
        Ok(blob.sha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> ExistingEntry {
        ExistingEntry {
            name: name.to_string(),
            is_dir: false,
        }
    }

    fn item(dir: &str, name: &str, on_exists: OnExists) -> Prepared {
        Prepared {
            dir: dir.to_string(),
            name: name.to_string(),
            on_exists,
        }
    }

    fn existing_in(dir: &str, names: &[&str]) -> HashMap<String, Vec<ExistingEntry>> {
        let mut map = HashMap::new();
        map.insert(dir.to_string(), names.iter().map(|n| entry(n)).collect());
        map
    }

    #[test]
    fn numbered_names_keep_the_extension() {
        assert_eq!(numbered("photo.jpg", 2), "photo (2).jpg");
        assert_eq!(numbered("報告書.v2.pdf", 3), "報告書.v2 (3).pdf");
        assert_eq!(numbered("README", 2), "README (2)");
        assert_eq!(numbered(".bashrc", 2), ".bashrc (2)");
    }

    #[test]
    fn new_names_are_written_as_they_are() {
        let items = [item("photos", "a.jpg", OnExists::Skip)];
        let decisions = resolve(&items, &HashMap::new()).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Write {
                path: "photos/a.jpg".to_string(),
                renamed: false
            }]
        );
    }

    #[test]
    fn skip_keeps_the_existing_file_and_is_reported() {
        let items = [item("", "a.txt", OnExists::Skip)];
        let decisions = resolve(&items, &existing_in("", &["a.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Skip {
                path: "a.txt".to_string()
            }]
        );
    }

    #[test]
    fn replace_uses_the_existing_spelling_of_the_name() {
        let items = [item("", "Report.TXT", OnExists::Replace)];
        let decisions = resolve(&items, &existing_in("", &["report.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Write {
                path: "report.txt".to_string(),
                renamed: false
            }]
        );
    }

    #[test]
    fn keep_both_avoids_existing_names_case_insensitively() {
        let items = [item("", "a.txt", OnExists::KeepBoth)];
        let existing = existing_in("", &["A.txt", "a (2).txt"]);
        let decisions = resolve(&items, &existing).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Write {
                path: "a (3).txt".to_string(),
                renamed: true
            }]
        );
    }

    #[test]
    fn keep_both_avoids_names_of_other_new_items_in_the_same_request() {
        // 既存の a.txt を両方残すと "a (2).txt" になるが、同じ要求の新規の項目がその名前を使う
        let items = [
            item("", "a.txt", OnExists::KeepBoth),
            item("", "a (2).txt", OnExists::Skip),
        ];
        let decisions = resolve(&items, &existing_in("", &["a.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![
                Decision::Write {
                    path: "a (3).txt".to_string(),
                    renamed: true
                },
                Decision::Write {
                    path: "a (2).txt".to_string(),
                    renamed: false
                },
            ]
        );
    }

    #[test]
    fn same_name_in_another_folder_is_not_a_conflict() {
        let items = [item("b", "a.txt", OnExists::Skip)];
        let decisions = resolve(&items, &existing_in("a", &["a.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Write {
                path: "b/a.txt".to_string(),
                renamed: false
            }]
        );
    }

    #[test]
    fn a_folder_with_the_same_name_is_refused() {
        let items = [item("", "docs", OnExists::Replace)];
        let mut existing = HashMap::new();
        existing.insert(
            String::new(),
            vec![ExistingEntry {
                name: "Docs".to_string(),
                is_dir: true,
            }],
        );
        let result = resolve(&items, &existing);
        assert!(matches!(result, Err(RemoteError::NameIsFolder(p)) if p == "docs"));
    }

    #[test]
    fn two_new_items_with_the_same_path_are_refused_case_insensitively() {
        let items = [
            item("", "a.txt", OnExists::Skip),
            item("", "A.TXT", OnExists::Skip),
        ];
        let result = resolve(&items, &HashMap::new());
        assert!(matches!(result, Err(RemoteError::DuplicateTarget(p)) if p == "A.TXT"));
    }

    #[test]
    fn two_items_replacing_the_same_file_are_refused() {
        let items = [
            item("", "a.txt", OnExists::Replace),
            item("", "A.txt", OnExists::Replace),
        ];
        let result = resolve(&items, &existing_in("", &["a.txt"]));
        assert!(matches!(result, Err(RemoteError::DuplicateTarget(_))));
    }

    #[test]
    fn skipped_duplicates_are_not_a_target_conflict() {
        let items = [
            item("", "a.txt", OnExists::Skip),
            item("", "a.txt", OnExists::Replace),
        ];
        let decisions = resolve(&items, &existing_in("", &["a.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![
                Decision::Skip {
                    path: "a.txt".to_string()
                },
                Decision::Write {
                    path: "a.txt".to_string(),
                    renamed: false
                },
            ]
        );
    }

    #[test]
    fn existing_names_stored_in_nfd_are_the_same_name() {
        // GitHub 側が NFD（か + 濁点）で持つ名前と、NFC で要求された名前は同じ
        let items = [item("", "が.txt", OnExists::Skip)];
        let decisions = resolve(&items, &existing_in("", &["か\u{3099}.txt"])).expect("ok");
        assert_eq!(
            decisions,
            vec![Decision::Skip {
                path: "か\u{3099}.txt".to_string()
            }]
        );
    }

    #[test]
    fn default_message_counts_the_saved_items() {
        assert_eq!(build_message(None, 3), "スマホから追加（3 件）");
        assert_eq!(build_message(Some("   "), 1), "スマホから追加（1 件）");
        assert_eq!(build_message(Some("写真"), 1), "写真");
        let long = "あ".repeat(600);
        assert_eq!(build_message(Some(&long), 1).chars().count(), 500);
    }

    #[test]
    fn sizes_are_checked_before_any_request() {
        assert!(matches!(
            check_sizes(0, std::iter::empty()),
            Err(RemoteError::EmptyRequest)
        ));
        assert!(matches!(
            check_sizes(MAX_ITEMS + 1, std::iter::repeat_n(1, MAX_ITEMS + 1)),
            Err(RemoteError::TooMany)
        ));
        assert!(matches!(
            check_sizes(1, std::iter::once(MAX_FILE_BYTES + 1)),
            Err(RemoteError::TooLarge)
        ));
        assert!(check_sizes(1, std::iter::once(MAX_FILE_BYTES)).is_ok());
        // 1 件ずつは上限内でも合計で超える
        let each = MAX_FILE_BYTES;
        assert!(matches!(
            check_sizes(3, std::iter::repeat_n(each, 3)),
            Err(RemoteError::TooLarge)
        ));
        assert!(check_sizes(2, std::iter::repeat_n(each, 2)).is_ok());
    }
}
