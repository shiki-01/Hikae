// 履歴一覧（設計書 4.2、6.2、9.3 の `list_history`）。
//
// - 手動の保存は `HEAD` の履歴、自動保存は `refs/hikae/snapshots/<ブランチ>/` の ref が指す commit
// - 出力は `-z` と専用の区切り文字で解析する（メモに `|` や改行が入っても壊れない）
// - ファイル数は第 1 親との差を `show --first-parent --name-only -z` で数える（ルートは全ファイルを
//   追加として数える）。ページ全体を少ない回数（150 件ずつ）の `show` にまとめ、commit ごとの起動はしない
// - 復元点のうち `refs/hikae/backup/` は履歴に出さない
// - メモ末尾のトレーラー `Hikae-PC: <PC 名>` は `pc_name` に分け、`message` には含めない

use crate::models::*;
use crate::operations::current_branch;
use crate::remote::unuploaded_commits;
use core_git::GitRunner;
use core_safety::SNAPSHOT_REF_PREFIX;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// `log` のフィールド区切り（US、メモには現れない制御文字）
const FIELD_SEP: char = '\u{1f}';

/// `log -z --format` の 1 件分
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogRecord {
    pub oid: String,
    /// 親 commit（先頭が第 1 親）
    pub parents: Vec<String>,
    pub tree: String,
    /// 作者日時（ISO 8601）
    pub timestamp: String,
    /// メモ全文（末尾の空白・改行は除く）
    pub message: String,
}

/// `log -z --format=%H%x1f%P%x1f%T%x1f%aI%x1f%B` の出力を解析する。
/// commit 同士は NUL で区切られ、フィールドは US（0x1f）で区切られる。
/// 形式が合わない断片は読み飛ばす。
pub(crate) fn parse_log_records(stdout: &[u8]) -> Vec<LogRecord> {
    let text = String::from_utf8_lossy(stdout);
    text.split('\0')
        .filter_map(|record| {
            // 先頭の改行は区切りの揺れ。メモ本体の改行は第 5 フィールドの中にだけある
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                return None;
            }
            let mut fields = record.splitn(5, FIELD_SEP);
            let oid = fields.next()?.trim();
            let parents = fields.next()?;
            let tree = fields.next()?.trim();
            let timestamp = fields.next()?.trim();
            let message = fields.next()?;
            if oid.is_empty() {
                return None;
            }
            Some(LogRecord {
                oid: oid.to_string(),
                parents: parents.split_whitespace().map(str::to_string).collect(),
                tree: tree.to_string(),
                timestamp: timestamp.to_string(),
                message: message.trim_end().to_string(),
            })
        })
        .collect()
}

/// 変更ファイル数の取得で 1 回の `show` に並べる commit の上限。
/// 引数が長すぎると OS のコマンドライン長の上限（Windows は約 32,000 文字）を超えるため、
/// 完全な OID（SHA-256 でも 64 文字）で 150 件（約 10,000 文字）ずつに分ける。
const COUNT_CHUNK: usize = 150;

/// 変更ファイル数の取得で、commit の見出しに使う開始記号（RS）。ファイル名には現れない制御文字
const HEADER_MARK: u8 = 0x1e;

/// `show --first-parent --name-only -z --format=%x1e%H%x1f` の出力から、commit ごとの
/// 変更ファイル数を数える。
///
/// 出力は commit ごとに `RS<OID>US NUL LF <ファイル> NUL <ファイル> NUL ...` と並ぶ。
/// 見出し（RS で始まるトークン）を区切りにして、次の見出しまでのファイル名の数を数える。
/// 見出しの直後のトークンに付く先頭の改行は区切りの揺れなので 1 つだけ取り除く。
/// メモは出力に含めないため、メモの内容（改行・`|` など）は結果に影響しない。
/// ルート commit は全ファイル、マージ commit は第 1 親との差になる（`--first-parent` による）。
pub(crate) fn parse_changed_file_counts(stdout: &[u8]) -> HashMap<String, u32> {
    let mut counts = HashMap::new();
    let mut current: Option<String> = None;
    let mut after_header = false;
    for raw in stdout.split(|b| *b == 0) {
        // 直前の commit にファイルが無いと、次の見出しにも先頭の改行が付く
        let stripped = raw.strip_prefix(b"\n").unwrap_or(raw);
        if stripped.first() == Some(&HEADER_MARK) {
            let body = &stripped[1..];
            let oid_end = body.iter().position(|b| *b == 0x1f);
            if let Some(end) = oid_end {
                let oid = String::from_utf8_lossy(&body[..end]).trim().to_string();
                if !oid.is_empty() {
                    counts.entry(oid.clone()).or_insert(0);
                    current = Some(oid);
                    after_header = true;
                    continue;
                }
            }
        }
        // 見出しの直後のトークンだけ、先頭の改行を取り除く
        let token = if after_header { stripped } else { raw };
        after_header = false;
        if token.is_empty() {
            continue;
        }
        if let Some(oid) = &current {
            *counts.entry(oid.clone()).or_insert(0) += 1;
        }
    }
    counts
}

/// 指定した commit それぞれの変更ファイル数を、少ない回数の `show` でまとめて取得する（読み取りのみ）。
///
/// commit ごとに `git` を起動すると Windows ではプロセスの起動が支配的になる（100 件で約 4 秒）。
/// `show` は列挙（walk）をせず、並べた commit だけを表示するため、`--first-parent` の付与で
/// マージ commit の差を第 1 親との差に揃えられる。許可リストに無い `log --no-walk` は使わない。
fn changed_file_counts(
    runner: &GitRunner,
    repo: &Path,
    oids: &[&str],
) -> Result<HashMap<String, u32>, OpsError> {
    let mut all = HashMap::new();
    for chunk in oids.chunks(COUNT_CHUNK) {
        let mut args: Vec<&str> = vec![
            "show",
            "--first-parent",
            "--name-only",
            "-z",
            "--format=%x1e%H%x1f",
        ];
        args.extend(chunk.iter().copied());
        args.push("--");
        let out = runner.run_ok(repo, &args)?;
        all.extend(parse_changed_file_counts(&out.stdout));
    }
    Ok(all)
}

/// 履歴の 1 回の取得で `git log` の引数に並べる自動保存の ref の上限。
/// 引数が長すぎると OS のコマンドライン長の上限（Windows は約 32,000 文字）を超えるため、
/// 新しい順にこの件数までに限る。ページ（offset / limit）によらず同じ集合を使うので、
/// ページを跨いでも並びは変わらない。
const SNAPSHOT_TIP_LIMIT: usize = 400;

/// 現在のブランチの自動保存（snapshot ref）を新しい順に `(commit, ref名)` で返す。
/// 取得できなければ（detached HEAD など）空にする。
fn snapshot_refs(
    runner: &GitRunner,
    repo: &Path,
    limit: usize,
) -> Result<Vec<(String, String)>, OpsError> {
    let Ok(branch) = current_branch(runner, repo) else {
        return Ok(Vec::new());
    };
    let pattern = format!("{SNAPSHOT_REF_PREFIX}{branch}/");
    let out = runner.run_ok(
        repo,
        &[
            "for-each-ref",
            "--sort=-refname",
            "--format=%(objectname)%09%(refname)",
            &pattern,
        ],
    )?;
    let mut refs = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((oid, name)) = line.split_once('\t') {
            if !oid.is_empty() && !name.is_empty() {
                refs.push((oid.to_string(), name.to_string()));
            }
        }
        if refs.len() >= limit {
            break;
        }
    }
    Ok(refs)
}

/// `HEAD` から辿れる手動の保存すべてのツリー ID。自動保存の重複判定に使う（読み取りのみ）。
fn manual_tree_ids(
    runner: &GitRunner,
    repo: &Path,
    snapshot_commits: &HashMap<&str, &str>,
) -> Result<HashSet<String>, OpsError> {
    let out = runner.run_ok(repo, &["log", "--format=%H %T", "HEAD", "--"])?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().split_once(' '))
        .filter(|(oid, tree)| {
            !oid.is_empty() && !tree.is_empty() && !snapshot_commits.contains_key(oid)
        })
        .map(|(_, tree)| tree.to_string())
        .collect())
}

/// 最新の手動の保存（`HEAD`）の日時（ISO 8601）。まだ保存が無ければ None（読み取りのみ）。
/// 自動保存は含めない。一覧のカードの「最終保存」に使う。
pub(crate) fn last_saved_at(runner: &GitRunner, repo: &Path) -> Result<Option<String>, OpsError> {
    let out = runner.run(
        repo,
        &["log", "--max-count=1", "--format=%aI", "HEAD", "--"],
    )?;
    if out.code != 0 {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!text.is_empty()).then_some(text))
}

/// 履歴一覧のうち、新しい順で `offset` 件目から最大 `limit` 件を取得する。
///
/// 手動の保存（`HEAD` から辿れる commit）と自動保存（snapshot ref の commit）を併せて返す。
/// 保存の直前に作られる復元点のように、同じ内容の手動の保存がある自動保存は重複するため出さない。
///
/// ページを跨いで重複・欠落が起きないよう、毎回「先頭から `offset + limit` 件」を同じ規則で
/// 数え直し、その末尾の `limit` 件を返す（`--skip` で読み飛ばすと、重複判定の対象が窓の位置で
/// 変わってしまう）。重複判定は窓の外の手動の保存も含めた全体で行うため、同じ自動保存が
/// ページによって出たり消えたりしない。変更ファイル数の集計（少ない回数の `git show`）は、
/// 返す `limit` 件だけに行う。
pub(crate) fn history(
    runner: &GitRunner,
    repo: &Path,
    offset: usize,
    limit: usize,
) -> Result<Vec<HistoryEntry>, OpsError> {
    // まだ 1 度も保存していないプロジェクトは空の履歴
    let head = runner.run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if head.code != 0 || limit == 0 {
        return Ok(Vec::new());
    }
    let wanted = offset.saturating_add(limit);

    let snapshots = snapshot_refs(runner, repo, SNAPSHOT_TIP_LIMIT)?;
    let ref_by_commit: HashMap<&str, &str> = snapshots
        .iter()
        .map(|(oid, name)| (oid.as_str(), name.as_str()))
        .collect();

    // 重複する自動保存を後で取り除くので、その分だけ多めに取得する
    let fetch = wanted.saturating_add(snapshots.len());
    let max_arg = format!("--max-count={fetch}");
    let mut args: Vec<&str> = vec![
        "log",
        "-z",
        "--format=%H%x1f%P%x1f%T%x1f%aI%x1f%B",
        &max_arg,
        "HEAD",
    ];
    args.extend(snapshots.iter().map(|(oid, _)| oid.as_str()));
    args.push("--");
    let out = runner.run_ok(repo, &args)?;
    let records = parse_log_records(&out.stdout);

    // クラウドに上がっていない保存（upstream が無ければ None）。手動の保存のうち、これに含まれず
    // upstream から辿れるものを「クラウドにある」とする
    let unuploaded = unuploaded_commits(runner, repo)?;

    // 手動の保存と同じ内容（ツリー）の自動保存は出さない。自動保存が無ければ判定は要らない
    let manual_trees = if snapshots.is_empty() {
        HashSet::new()
    } else {
        manual_tree_ids(runner, repo, &ref_by_commit)?
    };

    // 重複を除いた並びの、先頭から `wanted` 件
    let visible: Vec<(&LogRecord, Option<&str>)> = records
        .iter()
        .filter_map(|record| {
            let snapshot_ref = ref_by_commit.get(record.oid.as_str()).copied();
            if snapshot_ref.is_some() && manual_trees.contains(record.tree.as_str()) {
                return None;
            }
            Some((record, snapshot_ref))
        })
        .take(wanted)
        .collect();

    // 返す分の変更ファイル数を、commit ごとではなくまとめて取得する
    let page: Vec<(&LogRecord, Option<&str>)> = visible.into_iter().skip(offset).collect();
    let page_oids: Vec<&str> = page.iter().map(|(record, _)| record.oid.as_str()).collect();
    let counts = changed_file_counts(runner, repo, &page_oids)?;

    let mut entries = Vec::new();
    for (record, snapshot_ref) in page {
        let changed_files_count = counts.get(&record.oid).copied().unwrap_or(0);
        let cloud_synced = snapshot_ref.is_none()
            && unuploaded
                .as_ref()
                .is_some_and(|set| !set.contains(&record.oid));
        let kind = if snapshot_ref.is_some() {
            HistoryKind::Auto
        } else {
            HistoryKind::Manual
        };
        // メモ本文からは PC 名のトレーラーを除く
        let (message, pc_name) = crate::pc_name::split_trailer(&record.message);
        entries.push(HistoryEntry {
            commit: record.oid.chars().take(7).collect(),
            timestamp: record.timestamp.clone(),
            message,
            snapshot_ref: snapshot_ref.map(str::to_string),
            changed_files_count,
            kind,
            pc_name,
            cloud_synced,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_parser_keeps_pipes_and_newlines_in_messages() {
        let raw =
            "aaa\u{1f}bbb ccc\u{1f}ttt\u{1f}2026-10-07T12:00:00+09:00\u{1f}a | b\n2行目 | c\n\n\0\
                   ddd\u{1f}\u{1f}uuu\u{1f}2026-10-06T12:00:00+09:00\u{1f}root\n\0";
        let got = parse_log_records(raw.as_bytes());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].oid, "aaa");
        assert_eq!(got[0].parents, ["bbb", "ccc"]);
        assert_eq!(got[0].tree, "ttt");
        assert_eq!(got[0].message, "a | b\n2行目 | c");
        assert!(got[1].parents.is_empty());
        assert_eq!(got[1].message, "root");
    }

    #[test]
    fn log_parser_skips_garbage_and_empty_input() {
        assert!(parse_log_records(b"").is_empty());
        assert!(parse_log_records(b"\0\0").is_empty());
        assert!(parse_log_records(b"no separators here\0").is_empty());
    }

    #[test]
    fn file_count_parser_splits_by_headers() {
        // 2 件目はファイル無し（マージで差が無い場合）、3 件目の直前の見出しにも改行が付く
        let raw = "\u{1e}aaa\u{1f}\0\na.txt\0b c.txt\0\
                   \u{1e}bbb\u{1f}\0\n\
                   \u{1e}ccc\u{1f}\0\n日本語.txt\0";
        let got = parse_changed_file_counts(raw.as_bytes());
        assert_eq!(got.len(), 3);
        assert_eq!(got["aaa"], 2);
        assert_eq!(got["bbb"], 0);
        assert_eq!(got["ccc"], 1);
        assert!(parse_changed_file_counts(b"").is_empty());
        assert!(parse_changed_file_counts(b"\0\n\0").is_empty());
    }

    #[test]
    fn file_count_parser_is_not_confused_by_odd_file_names() {
        // `|`・タブ・US を含む名前も 1 件として数える（見出しは RS で始まるものだけ）
        let raw = "\u{1e}aaa\u{1f}\0\na|b.txt\0c\td.txt\0e\u{1f}f.txt\0";
        let got = parse_changed_file_counts(raw.as_bytes());
        assert_eq!(got["aaa"], 3);
    }
}
