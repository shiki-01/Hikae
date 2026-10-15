// 状態を持つ疑似 GitHub サーバー（統合テスト用）。
//
// tiny_http でスレッドを 1 本立て、メモリ上の blob・tree・commit・ref を REST の形で返す。
// 実際の GitHub の完全な再現ではない。クライアントが呼ぶ経路の範囲で、
// パスの入れ子・同名の扱い・fast-forward の判定を実際に動かして確かめるためのもの。
// サーバーは 1 本のスレッドで順に処理するので、「PATCH の直前に別の端末が更新する」フックも
// 決まった順序で動く。受けた全リクエストは（メソッド、パス、本文）で記録する。

#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use core_remote::RemoteClient;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use tiny_http::{Header, Response, Server};

pub const OWNER: &str = "alice";
pub const REPO: &str = "notes";
pub const DEFAULT_BRANCH: &str = "main";
/// 認証に使うトークンの値（`Authorization: Bearer ...` の形で届くことを確かめる）
pub const TOKEN_VALUE: &str = "test_token_value";
/// 既定の作者（alice）の noreply アドレス
pub const ALICE_NOREPLY: &str = "12345+alice@users.noreply.github.com";
/// 別の端末（他の人の更新に見立てる）の作者
const OTHER_NAME: &str = "bob";
const OTHER_EMAIL: &str = "2222+bob@users.noreply.github.com";
/// 初期のコミットの作成日時
const INITIAL_DATE: &str = "2026-10-01T00:00:00Z";
/// 初期のコミットのメッセージ
pub const INITIAL_MESSAGE: &str = "初期の版";
/// 別の端末の更新のメッセージ
pub const REMOTE_MESSAGE: &str = "別の端末の更新";

/// 作者・committer
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub date: String,
}

/// ツリーの 1 項目（blob または tree）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// `blob` または `tree`
    pub kind: String,
    pub mode: String,
    pub sha: String,
}

/// ツリー（名前 → 項目）
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: BTreeMap<String, Node>,
}

/// コミット。親は 1 つ（または 0 個）
#[derive(Clone, Debug)]
pub struct Commit {
    pub tree: String,
    pub parents: Vec<String>,
    pub message: String,
    pub author: Person,
    pub committer: Person,
}

/// 受けたリクエストの記録
#[derive(Clone, Debug)]
pub struct Logged {
    pub method: String,
    /// クエリを除いた生のパス
    pub path: String,
    /// JSON として読めた本文（読めなければ None）
    pub body: Option<Value>,
}

/// サーバーの状態（1 つの mutex の中）
struct State {
    /// ID の連番（40 桁の 16 進にする）
    seq: u64,
    blobs: HashMap<String, Vec<u8>>,
    trees: HashMap<String, Tree>,
    commits: HashMap<String, Commit>,
    /// 既定ブランチ（main）の先頭
    head: String,
    log: Vec<Logged>,
    /// 次の PATCH の直前に、別の端末が (パス, 内容) を足した commit を作って ref を進める
    queued_remote: VecDeque<(String, Vec<u8>)>,
    /// 500 を返す blob の POST の番号（サーバー起動からの 1 始まり）
    fail_blob_posts: Vec<usize>,
    blob_post_seen: usize,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 連番から作る日時（秒単位で進む）
fn date_at(seq: u64) -> String {
    format!(
        "2026-10-01T{:02}:{:02}:{:02}Z",
        (seq / 3600) % 24,
        (seq / 60) % 60,
        seq % 60
    )
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

fn file_node(sha: String) -> Node {
    Node {
        kind: "blob".to_string(),
        mode: "100644".to_string(),
        sha,
    }
}

fn tree_node(sha: String) -> Node {
    Node {
        kind: "tree".to_string(),
        mode: "040000".to_string(),
        sha,
    }
}

impl State {
    fn new() -> Self {
        let mut st = State {
            seq: 0,
            blobs: HashMap::new(),
            trees: HashMap::new(),
            commits: HashMap::new(),
            head: String::new(),
            log: Vec::new(),
            queued_remote: VecDeque::new(),
            fail_blob_posts: Vec::new(),
            blob_post_seen: 0,
        };
        // 初期の状態: README.md、docs/a.txt、日本語名のフォルダの資料/議事録.txt
        let files: [(&str, &[u8]); 3] = [
            ("README.md", b"# notes\n"),
            ("docs/a.txt", b"alpha\n"),
            ("資料/議事録.txt", "議事録の本文\n".as_bytes()),
        ];
        let mut root: Option<String> = None;
        for (path, bytes) in files {
            let blob = st.put_blob(bytes.to_vec());
            let segs: Vec<&str> = path.split('/').collect();
            root = insert_segs(&mut st, root.as_deref(), &segs, file_node(blob));
        }
        let root = root.expect("初期のツリー");
        let author = person(OWNER, ALICE_NOREPLY, INITIAL_DATE);
        let head = st.store_commit(
            root,
            Vec::new(),
            INITIAL_MESSAGE.to_string(),
            author.clone(),
            author,
        );
        st.head = head;
        st
    }

    fn next_id(&mut self) -> String {
        self.seq += 1;
        format!("{:040x}", self.seq)
    }

    fn put_blob(&mut self, bytes: Vec<u8>) -> String {
        let id = self.next_id();
        self.blobs.insert(id.clone(), bytes);
        id
    }

    fn store_tree(&mut self, tree: Tree) -> String {
        let id = self.next_id();
        self.trees.insert(id.clone(), tree);
        id
    }

    fn store_commit(
        &mut self,
        tree: String,
        parents: Vec<String>,
        message: String,
        author: Person,
        committer: Person,
    ) -> String {
        let id = self.next_id();
        self.commits.insert(
            id.clone(),
            Commit {
                tree,
                parents,
                message,
                author,
                committer,
            },
        );
        id
    }

    /// ツリーの中の項目を辿る（`segs` が空ならルートのツリーそのもの）
    fn node_at(&self, root: &str, segs: &[&str]) -> Option<Node> {
        let mut current = tree_node(root.to_string());
        for seg in segs {
            if current.kind != "tree" {
                return None;
            }
            let tree = self.trees.get(&current.sha)?;
            current = tree.nodes.get(*seg)?.clone();
        }
        Some(current)
    }

    /// コミットの時点のパスの blob の ID（ファイルでなければ None）
    fn blob_at(&self, commit: &str, path: &str) -> Option<String> {
        let c = self.commits.get(commit)?;
        let segs: Vec<&str> = path.split('/').collect();
        let node = self.node_at(&c.tree, &segs)?;
        (node.kind == "blob").then_some(node.sha)
    }

    /// ツリーの中のファイルを全部（パス → blob の ID）で集める
    fn flatten(&self, tree_id: &str, prefix: &str, out: &mut BTreeMap<String, String>) {
        let Some(tree) = self.trees.get(tree_id) else {
            return;
        };
        for (name, node) in &tree.nodes {
            let path = join(prefix, name);
            if node.kind == "tree" {
                self.flatten(&node.sha, &path, out);
            } else {
                out.insert(path, node.sha.clone());
            }
        }
    }

    /// 別の端末の更新（先頭の commit に、指定のファイルを足した commit を作って ref を進める）
    fn remote_update(&mut self, path: &str, content: &[u8]) {
        let blob = self.put_blob(content.to_vec());
        let base = self.commits.get(&self.head).map(|c| c.tree.clone());
        let segs: Vec<&str> = path.split('/').collect();
        let tree = insert_segs(self, base.as_deref(), &segs, file_node(blob))
            .expect("別の端末の更新のツリー");
        let other = person(OTHER_NAME, OTHER_EMAIL, &date_at(self.seq));
        let parent = self.head.clone();
        self.head = self.store_commit(
            tree,
            vec![parent],
            REMOTE_MESSAGE.to_string(),
            other.clone(),
            other,
        );
    }
}

fn person(name: &str, email: &str, date: &str) -> Person {
    Person {
        name: name.to_string(),
        email: email.to_string(),
        date: date.to_string(),
    }
}

/// ベースのツリーに、パス（`segs`）の位置へ項目を載せた新しいツリーを作る。
/// 同名は置き換え、無い階層は作る。途中が blob なら None。
fn insert_segs(st: &mut State, base: Option<&str>, segs: &[&str], node: Node) -> Option<String> {
    let (first, rest) = segs.split_first()?;
    let mut tree = match base {
        Some(id) => st.trees.get(id)?.clone(),
        None => Tree::default(),
    };
    if rest.is_empty() {
        tree.nodes.insert((*first).to_string(), node);
    } else {
        let child = match tree.nodes.get(*first) {
            Some(n) if n.kind == "tree" => Some(n.sha.clone()),
            Some(_) => return None,
            None => None,
        };
        let sub = insert_segs(st, child.as_deref(), rest, node)?;
        tree.nodes.insert((*first).to_string(), tree_node(sub));
    }
    Some(st.store_tree(tree))
}

/// 疑似サーバー本体。`Drop` でスレッドを止める。
pub struct FakeGithub {
    server: Arc<Server>,
    state: Arc<Mutex<State>>,
    base: String,
    handle: Option<JoinHandle<()>>,
}

impl FakeGithub {
    /// 127.0.0.1 の空いているポートで起動する。初期の状態は README.md・docs/a.txt・資料/議事録.txt の 1 コミット。
    pub fn start() -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").expect("疑似サーバーの起動"));
        let port = server
            .server_addr()
            .to_ip()
            .expect("待ち受けのアドレス")
            .port();
        let state = Arc::new(Mutex::new(State::new()));
        let handle = {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            std::thread::spawn(move || serve(server, state))
        };
        Self {
            server,
            state,
            base: format!("http://127.0.0.1:{port}"),
            handle: Some(handle),
        }
    }

    /// `RemoteClient::with_base` に渡す基点
    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub fn client(&self) -> RemoteClient {
        RemoteClient::with_base(self.base.clone())
    }

    /// 既定ブランチの先頭
    pub fn head(&self) -> String {
        lock(&self.state).head.clone()
    }

    /// 受けた全リクエストの記録（受理・拒否を問わない）
    pub fn requests(&self) -> Vec<Logged> {
        lock(&self.state).log.clone()
    }

    /// 次の PATCH の直前に、別の端末が (パス, 内容) を足す。呼ぶたびに 1 回分。
    pub fn queue_remote_update(&self, path: &str, content: &[u8]) {
        lock(&self.state)
            .queued_remote
            .push_back((path.to_string(), content.to_vec()));
    }

    /// サーバー起動から `nth` 回目（1 始まり）の blob の POST を 500 で失敗させる。
    pub fn fail_blob_post(&self, nth: usize) {
        lock(&self.state).fail_blob_posts.push(nth);
    }

    /// コミットの中身（テストからの直接の確認用）
    pub fn commit(&self, sha: &str) -> Option<Commit> {
        lock(&self.state).commits.get(sha).cloned()
    }

    /// コミットの時点のツリーにある全ファイル（パス → blob の ID）
    pub fn tree_files(&self, commit: &str) -> BTreeMap<String, String> {
        let st = lock(&self.state);
        let mut out = BTreeMap::new();
        if let Some(c) = st.commits.get(commit) {
            st.flatten(&c.tree, "", &mut out);
        }
        out
    }

    /// コミットの時点のファイルの中身
    pub fn file_bytes(&self, commit: &str, path: &str) -> Option<Vec<u8>> {
        let st = lock(&self.state);
        let sha = st.blob_at(commit, path)?;
        st.blobs.get(&sha).cloned()
    }

    /// 先頭から最初の親をたどった commit の列（新しい順、先頭を含む）
    pub fn first_parent_chain(&self, from: &str) -> Vec<String> {
        let st = lock(&self.state);
        let mut out = Vec::new();
        let mut cur = Some(from.to_string());
        while let Some(id) = cur {
            let Some(c) = st.commits.get(&id) else {
                break;
            };
            cur = c.parents.first().cloned();
            out.push(id);
        }
        out
    }

    /// 記録のうち、メソッドが一致し、パスが `suffix` で終わるものの数
    pub fn count_requests(&self, method: &str, suffix: &str) -> usize {
        lock(&self.state)
            .log
            .iter()
            .filter(|r| r.method == method && r.path.ends_with(suffix))
            .count()
    }
}

impl Drop for FakeGithub {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve(server: Arc<Server>, state: Arc<Mutex<State>>) {
    let expected = format!("Bearer {TOKEN_VALUE}");
    for mut request in server.incoming_requests() {
        let method = request.method().as_str().to_string();
        let url = request.url().to_string();
        let auth_ok = request
            .headers()
            .iter()
            .any(|h| h.field.equiv("Authorization") && h.value.as_str() == expected);
        let mut raw = Vec::new();
        let _ = request.as_reader().read_to_end(&mut raw);
        let body: Option<Value> = if raw.is_empty() {
            None
        } else {
            serde_json::from_slice(&raw).ok()
        };
        let (path, query) = split_url(&url);
        let (status, reply) = {
            let mut st = lock(&state);
            st.log.push(Logged {
                method: method.clone(),
                path: path.clone(),
                body: body.clone(),
            });
            route(
                &mut st,
                &method,
                &path,
                &query,
                auth_ok,
                &body.unwrap_or(Value::Null),
            )
        };
        let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("Content-Type");
        let response = Response::from_string(reply.to_string())
            .with_status_code(status)
            .with_header(header);
        let _ = request.respond(response);
    }
}

/// パスとクエリに分ける。クエリの値は復号する。
fn split_url(url: &str) -> (String, Vec<(String, String)>) {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    let pairs = query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    (path.to_string(), pairs)
}

/// `%XX` を復号する（UTF-8 のバイト列として解釈する）
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_value(query: &[(String, String)], key: &str) -> Option<String> {
    query.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn not_found() -> (u16, Value) {
    (404, json!({"message": "Not Found"}))
}

fn method_not_allowed() -> (u16, Value) {
    (405, json!({"message": "Method Not Allowed"}))
}

fn unprocessable(message: &str) -> (u16, Value) {
    (422, json!({"message": message}))
}

fn repo_json() -> Value {
    json!({
        "name": REPO,
        "owner": {"login": OWNER},
        "private": true,
        "default_branch": DEFAULT_BRANCH,
        "pushed_at": INITIAL_DATE,
        "description": "fake",
    })
}

fn person_json(p: &Person) -> Value {
    json!({"name": p.name, "email": p.email, "date": p.date})
}

fn commit_json(id: &str, c: &Commit) -> Value {
    let parents: Vec<Value> = c.parents.iter().map(|p| json!({"sha": p})).collect();
    json!({
        "sha": id,
        "tree": {"sha": c.tree},
        "parents": parents,
        "message": c.message,
        "author": person_json(&c.author),
        "committer": person_json(&c.committer),
    })
}

/// 本文の作者・committer。無ければ既定（alice）、日付が無ければ連番から作る。
fn person_from(v: &Value, seq: u64) -> Person {
    Person {
        name: v["name"].as_str().unwrap_or_default().to_string(),
        email: v["email"].as_str().unwrap_or_default().to_string(),
        date: v["date"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| date_at(seq)),
    }
}

fn route(
    st: &mut State,
    method: &str,
    path: &str,
    query: &[(String, String)],
    auth_ok: bool,
    body: &Value,
) -> (u16, Value) {
    if !auth_ok {
        return (401, json!({"message": "Bad credentials"}));
    }
    let segs: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .map(percent_decode)
        .collect();
    let s: Vec<&str> = segs.iter().map(String::as_str).collect();
    match s.as_slice() {
        ["user", "repos"] => {
            if method != "GET" {
                return method_not_allowed();
            }
            (200, json!([repo_json()]))
        }
        ["repos", o, n, rest @ ..] => {
            if *o != OWNER || *n != REPO {
                return not_found();
            }
            repo_route(st, method, rest, query, body)
        }
        _ => not_found(),
    }
}

/// `repos/{o}/{n}` の下の経路。許可する方法を先に決め、違えば 405、経路が無ければ 404。
fn repo_route(
    st: &mut State,
    method: &str,
    rest: &[&str],
    query: &[(String, String)],
    body: &Value,
) -> (u16, Value) {
    let allowed = match rest {
        [] => "GET",
        ["git", "ref", "heads", ..] => "GET",
        ["git", "refs", "heads", ..] => "PATCH",
        ["git", "commits", _] => "GET",
        ["git", "commits"] => "POST",
        ["git", "blobs", _] => "GET",
        ["git", "blobs"] => "POST",
        ["git", "trees"] => "POST",
        ["contents", ..] => "GET",
        ["commits"] => "GET",
        _ => return not_found(),
    };
    if allowed != method {
        return method_not_allowed();
    }
    match rest {
        [] => (200, repo_json()),
        ["git", "ref", "heads", b @ ..] => get_ref(st, &b.join("/")),
        ["git", "refs", "heads", b @ ..] => update_ref(st, &b.join("/"), body),
        ["git", "commits", sha] => match st.commits.get(*sha) {
            Some(c) => (200, commit_json(sha, c)),
            None => not_found(),
        },
        ["git", "commits"] => create_commit(st, body),
        ["git", "blobs", sha] => get_blob(st, sha),
        ["git", "blobs"] => create_blob(st, body),
        ["git", "trees"] => create_tree(st, body),
        ["contents", p @ ..] => contents(st, p, query),
        ["commits"] => list_commits(st, query),
        _ => not_found(),
    }
}

fn get_ref(st: &State, branch: &str) -> (u16, Value) {
    if branch != DEFAULT_BRANCH {
        return not_found();
    }
    (
        200,
        json!({
            "ref": format!("refs/heads/{branch}"),
            "object": {"sha": st.head, "type": "commit"},
        }),
    )
}

/// PATCH の ref の更新。`force` が true なら拒否する。直前に別の端末の更新を入れる（キューがあれば）。
/// 新しい commit の親に現在の先頭が含まれるときだけ進める（fast-forward）。
fn update_ref(st: &mut State, branch: &str, body: &Value) -> (u16, Value) {
    if branch != DEFAULT_BRANCH {
        return not_found();
    }
    if body["force"].as_bool() == Some(true) {
        return unprocessable("force は疑似サーバーでは拒否する");
    }
    let Some(sha) = body["sha"].as_str().map(str::to_string) else {
        return unprocessable("sha が必要");
    };
    if let Some((path, content)) = st.queued_remote.pop_front() {
        st.remote_update(&path, &content);
    }
    let Some(commit) = st.commits.get(&sha) else {
        return unprocessable("Object does not exist");
    };
    if commit.parents.contains(&st.head) {
        st.head = sha;
        (
            200,
            json!({
                "ref": format!("refs/heads/{DEFAULT_BRANCH}"),
                "object": {"sha": st.head, "type": "commit"},
            }),
        )
    } else {
        unprocessable("Update is not a fast forward")
    }
}

fn create_blob(st: &mut State, body: &Value) -> (u16, Value) {
    st.blob_post_seen += 1;
    if st.fail_blob_posts.contains(&st.blob_post_seen) {
        return (500, json!({"message": "疑似的な障害（blob の作成）"}));
    }
    if body["encoding"] != "base64" {
        return unprocessable("encoding は base64 のみ");
    }
    let Some(content) = body["content"].as_str() else {
        return unprocessable("content が必要");
    };
    let Ok(bytes) = STANDARD.decode(content) else {
        return unprocessable("base64 として読めない");
    };
    let id = st.put_blob(bytes);
    (201, json!({"sha": id}))
}

fn get_blob(st: &State, sha: &str) -> (u16, Value) {
    let Some(bytes) = st.blobs.get(sha) else {
        return not_found();
    };
    let encoded = STANDARD.encode(bytes);
    // GitHub は base64 を 60 桁ごとに改行して返す
    let wrapped = encoded
        .as_bytes()
        .chunks(60)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    (
        200,
        json!({
            "sha": sha,
            "size": bytes.len(),
            "encoding": "base64",
            "content": wrapped,
        }),
    )
}

fn create_tree(st: &mut State, body: &Value) -> (u16, Value) {
    let mut root: Option<String> = match body.get("base_tree") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_str() {
            Some(id) if st.trees.contains_key(id) => Some(id.to_string()),
            _ => return unprocessable("base_tree が存在しない"),
        },
    };
    let Some(items) = body["tree"].as_array() else {
        return unprocessable("tree が必要");
    };
    for item in items {
        let (Some(path), Some(kind), Some(sha), Some(mode)) = (
            item["path"].as_str(),
            item["type"].as_str(),
            item["sha"].as_str(),
            item["mode"].as_str(),
        ) else {
            return unprocessable("tree の項目が不完全");
        };
        let exists = match kind {
            "blob" => st.blobs.contains_key(sha),
            "tree" => st.trees.contains_key(sha),
            _ => false,
        };
        if !exists {
            return unprocessable("tree の項目の ID が存在しない");
        }
        let segs: Vec<&str> = path.split('/').collect();
        if segs.iter().any(|s| s.is_empty()) {
            return unprocessable("パスが正しくない");
        }
        let node = Node {
            kind: kind.to_string(),
            mode: mode.to_string(),
            sha: sha.to_string(),
        };
        root = match insert_segs(st, root.as_deref(), &segs, node) {
            Some(id) => Some(id),
            None => return unprocessable("途中の階層がファイルになっている"),
        };
    }
    let id = match root {
        Some(id) => id,
        None => st.store_tree(Tree::default()),
    };
    (201, json!({"sha": id}))
}

fn create_commit(st: &mut State, body: &Value) -> (u16, Value) {
    let Some(tree) = body["tree"].as_str().map(str::to_string) else {
        return unprocessable("tree が必要");
    };
    if !st.trees.contains_key(&tree) {
        return unprocessable("tree が存在しない");
    }
    let Some(parent_values) = body["parents"].as_array() else {
        return unprocessable("parents が必要");
    };
    let mut parents = Vec::with_capacity(parent_values.len());
    for p in parent_values {
        match p.as_str() {
            Some(id) if st.commits.contains_key(id) => parents.push(id.to_string()),
            _ => return unprocessable("親の commit が存在しない"),
        }
    }
    let Some(message) = body["message"].as_str().map(str::to_string) else {
        return unprocessable("message が必要");
    };
    let default = person(OWNER, ALICE_NOREPLY, "");
    let author = if body["author"].is_null() {
        default.clone()
    } else {
        person_from(&body["author"], st.seq)
    };
    let committer = if body["committer"].is_null() {
        default
    } else {
        person_from(&body["committer"], st.seq)
    };
    let id = st.store_commit(tree, parents, message, author, committer);
    let Some(c) = st.commits.get(&id) else {
        return unprocessable("commit の作成に失敗");
    };
    (201, commit_json(&id, c))
}

/// フォルダの中身（contents API の配列）
fn listing(st: &State, tree_id: &str, prefix: &str) -> Vec<Value> {
    let Some(tree) = st.trees.get(tree_id) else {
        return Vec::new();
    };
    tree.nodes
        .iter()
        .map(|(name, node)| {
            let path = join(prefix, name);
            let (kind, size) = if node.kind == "tree" {
                ("dir", 0)
            } else {
                ("file", st.blobs.get(&node.sha).map_or(0, Vec::len))
            };
            json!({
                "name": name,
                "path": path,
                "sha": node.sha,
                "size": size,
                "type": kind,
            })
        })
        .collect()
}

/// `contents/{path}?ref=`。フォルダなら配列、ファイルなら 1 件のオブジェクト、無ければ 404。
fn contents(st: &State, segs: &[&str], query: &[(String, String)]) -> (u16, Value) {
    let git_ref = query_value(query, "ref").unwrap_or_else(|| st.head.clone());
    let Some(commit) = st.commits.get(&git_ref) else {
        return not_found();
    };
    if segs.is_empty() {
        return (200, Value::Array(listing(st, &commit.tree, "")));
    }
    let prefix = segs.join("/");
    match st.node_at(&commit.tree, segs) {
        None => not_found(),
        Some(node) if node.kind == "tree" => (200, Value::Array(listing(st, &node.sha, &prefix))),
        Some(node) => {
            let size = st.blobs.get(&node.sha).map_or(0, Vec::len);
            let name = segs.last().copied().unwrap_or_default();
            (
                200,
                json!({
                    "name": name,
                    "path": prefix,
                    "sha": node.sha,
                    "size": size,
                    "type": "file",
                }),
            )
        }
    }
}

/// `commits?path=&per_page=`。先頭から親をたどり、そのパスの blob が親と違う commit（親が無ければ、そのパスがある commit）を新しい順に返す。
fn list_commits(st: &State, query: &[(String, String)]) -> (u16, Value) {
    let path = query_value(query, "path").filter(|p| !p.is_empty());
    let per_page = query_value(query, "per_page")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30)
        .min(100);
    let mut out = Vec::new();
    let mut cur = Some(st.head.clone());
    while let Some(id) = cur {
        let Some(c) = st.commits.get(&id) else {
            break;
        };
        let include = match &path {
            None => true,
            Some(p) => {
                let here = st.blob_at(&id, p);
                match c.parents.first() {
                    Some(parent) => here != st.blob_at(parent, p),
                    None => here.is_some(),
                }
            }
        };
        cur = c.parents.first().cloned();
        if include {
            out.push(json!({
                "sha": id,
                "commit": {
                    "message": c.message,
                    "author": {"name": c.author.name, "date": c.author.date},
                },
            }));
            if out.len() >= per_page {
                break;
            }
        }
    }
    (200, Value::Array(out))
}

/// 呼び出しの形の許可（`allow.rs` の許可リストと同じ形）。許可するものは `Some`。
fn shape(method: &str, path: &str) -> Option<()> {
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    let ok = matches!(
        (method, segs.as_slice()),
        ("GET", ["user", "repos"])
            | ("GET", ["repos", _, _])
            | ("GET", ["repos", _, _, "git", "ref", "heads", ..])
            | ("GET", ["repos", _, _, "git", "commits", _])
            | ("GET", ["repos", _, _, "git", "blobs", _])
            | (
                "POST",
                ["repos", _, _, "git", "blobs" | "trees" | "commits"]
            )
            | ("PATCH", ["repos", _, _, "git", "refs", "heads", ..])
            | ("GET", ["repos", _, _, "contents", ..])
            | ("GET", ["repos", _, _, "commits"])
    );
    ok.then_some(())
}

/// PATCH の本文が `{"sha": <文字列>, "force": false}` の 2 項目だけか
fn patch_body_ok(body: Option<&Value>) -> bool {
    match body {
        Some(Value::Object(map)) => {
            map.len() == 2
                && map.get("sha").is_some_and(Value::is_string)
                && map.get("force") == Some(&Value::Bool(false))
        }
        _ => false,
    }
}

/// 受けたリクエストの記録の規則違反を列挙する（空なら違反なし）。
///
/// - DELETE など、GET・POST・PATCH 以外のメソッドが 1 件もないこと
/// - PATCH は `git/refs/heads/` だけ、本文は `sha` と `force: false` だけ
/// - POST は blobs・trees・commits だけ
/// - すべての呼び出しが、許可リストと同じ形のどれかであること
pub fn violations(log: &[Logged]) -> Vec<String> {
    let mut out = Vec::new();
    for r in log {
        let id = format!("{} {}", r.method, r.path);
        if !matches!(r.method.as_str(), "GET" | "POST" | "PATCH") {
            out.push(format!("禁止されたメソッド: {id}"));
            continue;
        }
        if shape(&r.method, &r.path).is_none() {
            out.push(format!("許可されていない形の呼び出し: {id}"));
        }
        if r.method == "PATCH" {
            if !r.path.contains("/git/refs/heads/") {
                out.push(format!("PATCH が heads 以外に送られた: {id}"));
            }
            if !patch_body_ok(r.body.as_ref()) {
                out.push(format!(
                    "PATCH の本文が {{sha, force: false}} ではない: {id} {:?}",
                    r.body
                ));
            }
        }
        if r.method == "POST"
            && !(r.path.ends_with("/git/blobs")
                || r.path.ends_with("/git/trees")
                || r.path.ends_with("/git/commits"))
        {
            out.push(format!(
                "POST が blobs・trees・commits 以外に送られた: {id}"
            ));
        }
    }
    out
}

/// 規則違反が無いことを確かめる（各テストの最後に呼ぶ）
pub fn assert_request_rules(fake: &FakeGithub) {
    let v = violations(&fake.requests());
    assert!(v.is_empty(), "規則違反のリクエストがあります: {v:#?}");
}
