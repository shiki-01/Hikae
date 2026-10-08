// 履歴一覧（設計書 4.2、6.2、9.3 の `list_history`）。
//
// - 手動の保存は `HEAD` の履歴、自動保存は `refs/hikae/snapshots/<ブランチ>/` の ref が指す commit
// - 出力は `-z` と専用の区切り文字で解析する（メモに `|` や改行が入っても壊れない）
// - ファイル数は親との `diff --numstat -z` で数える（ルートは全ファイルを追加として数える）
// - 復元点のうち `refs/hikae/backup/` は履歴に出さない

use crate::models::*;
use crate::operations::current_branch;
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

/// `diff --numstat -z -M` の出力から変更ファイル数を数える。
/// 通常は `<追加>\t<削除>\t<パス>\0`、名前変更は `<追加>\t<削除>\t\0<元>\0<先>\0`（1 件として数える）。
pub(crate) fn count_numstat_files(stdout: &[u8]) -> u32 {
    let mut tokens = stdout.split(|b| *b == 0);
    let mut count = 0;
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        let mut parts = token.splitn(3, |b| *b == b'\t');
        let (Some(_), Some(_), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        count += 1;
        if path.is_empty() {
            // 名前変更・コピー: 続く 2 トークン（元・先）を読み飛ばす
            tokens.next();
            tokens.next();
        }
    }
    count
}

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

/// 履歴一覧を取得する。新しい順。
///
/// 手動の保存（`HEAD` から辿れる commit）と自動保存（snapshot ref の commit）を併せて返す。
/// 保存の直前に作られる復元点のように、同じ内容の手動の保存がある自動保存は重複するため出さない。
pub(crate) fn history(
    runner: &GitRunner,
    repo: &Path,
    max_count: usize,
) -> Result<Vec<HistoryEntry>, OpsError> {
    // まだ 1 度も保存していないプロジェクトは空の履歴
    let head = runner.run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if head.code != 0 || max_count == 0 {
        return Ok(Vec::new());
    }

    let snapshots = snapshot_refs(runner, repo, max_count)?;
    let ref_by_commit: HashMap<&str, &str> = snapshots
        .iter()
        .map(|(oid, name)| (oid.as_str(), name.as_str()))
        .collect();

    // 重複する自動保存を後で取り除くので、その分だけ多めに取得する
    let fetch = max_count.saturating_add(snapshots.len());
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

    // 手動の保存と同じ内容（ツリー）の自動保存は出さない
    let manual_trees: HashSet<&str> = records
        .iter()
        .filter(|r| !ref_by_commit.contains_key(r.oid.as_str()))
        .map(|r| r.tree.as_str())
        .collect();

    let mut entries = Vec::new();
    for record in &records {
        let snapshot_ref = ref_by_commit
            .get(record.oid.as_str())
            .map(|r| r.to_string());
        if snapshot_ref.is_some() && manual_trees.contains(record.tree.as_str()) {
            continue;
        }
        if entries.len() >= max_count {
            break;
        }

        let changed_files_count = match record.parents.first() {
            Some(parent) => {
                let stat = runner.run_ok(
                    repo,
                    &["diff", "--numstat", "-z", "-M", parent, &record.oid, "--"],
                )?;
                count_numstat_files(&stat.stdout)
            }
            None => {
                // ルート commit は比べる親が無い（空ツリーは diff の対象にできない）ため、
                // その時点の全ファイルを「追加」として数える
                let tree =
                    runner.run_ok(repo, &["ls-tree", "-r", "-z", "--name-only", &record.oid])?;
                tree.stdout
                    .split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .count() as u32
            }
        };
        let kind = if snapshot_ref.is_some() {
            HistoryKind::Auto
        } else {
            HistoryKind::Manual
        };
        entries.push(HistoryEntry {
            commit: record.oid.chars().take(7).collect(),
            timestamp: record.timestamp.clone(),
            message: record.message.clone(),
            snapshot_ref,
            changed_files_count,
            kind,
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
    fn numstat_counter_counts_renames_once() {
        let raw = b"1\t0\ta.txt\0-\t-\tb c.bin\0\
                    0\t0\t\0old name.txt\0new name.txt\0\
                    3\t4\t\xe6\x97\xa5\xe6\x9c\xac.txt\0";
        assert_eq!(count_numstat_files(raw), 4);
        assert_eq!(count_numstat_files(b""), 0);
        assert_eq!(count_numstat_files(b"\0"), 0);
    }
}
