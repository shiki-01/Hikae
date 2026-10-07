//! `git status --porcelain=v2 -z --branch` の出力パーサ。
//!
//! `-z` 形式ではエントリは NUL 区切りで、パスは引用符で囲まれず、行内のフィールドは空白区切り。
//! rename / copy（`2` エントリ）のみ、元のパスが次の NUL 区切りトークンとして続く。

use serde::{Deserialize, Serialize};

/// XY の 1 文字分の状態（porcelain v2 では未変更は `.`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusCode {
    Unmodified,
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    UpdatedButUnmerged,
}

impl StatusCode {
    fn from_char(c: char) -> Option<Self> {
        Some(match c {
            '.' => Self::Unmodified,
            'M' => Self::Modified,
            'T' => Self::TypeChanged,
            'A' => Self::Added,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'U' => Self::UpdatedButUnmerged,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusKind {
    /// 通常の変更（`1`）。`index` がステージ済み、`worktree` が未ステージ側
    Change {
        index: StatusCode,
        worktree: StatusCode,
    },
    /// rename（`R`）または copy（`C`）（`2`）。`score` は類似度（%）
    Rename {
        index: StatusCode,
        worktree: StatusCode,
        copy: bool,
        score: u32,
        original_path: String,
    },
    /// 競合（`u`）。`xy` は `UU` / `AA` / `DU` / `UD` / `DD` / `AU` / `UA` のいずれか
    Unmerged { xy: String },
    /// 未追跡（`?`）
    Untracked,
    /// 無視（`!`）
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusEntry {
    pub kind: StatusKind,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub oid: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    /// (ahead, behind)
    pub ab: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusV2 {
    pub branch: BranchInfo,
    pub entries: Vec<StatusEntry>,
}

impl StatusV2 {
    /// 競合しているエントリだけを返す
    pub fn unmerged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.kind, StatusKind::Unmerged { .. }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "status parse error: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

fn err(message: impl Into<String>) -> ParseError {
    ParseError {
        message: message.into(),
    }
}

fn parse_xy(xy: &str) -> Result<(StatusCode, StatusCode), ParseError> {
    let mut chars = xy.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(x), Some(y), None) => match (StatusCode::from_char(x), StatusCode::from_char(y)) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => Err(err(format!("invalid XY: {xy}"))),
        },
        _ => Err(err(format!("invalid XY: {xy}"))),
    }
}

/// `git status --porcelain=v2 -z --branch` の出力を解析する。パスが UTF-8 でなければエラー。
pub fn parse_status_v2(stdout: &[u8]) -> Result<StatusV2, ParseError> {
    let mut tokens = stdout.split(|&b| b == 0).filter(|t| !t.is_empty());
    let mut status = StatusV2::default();

    while let Some(raw) = tokens.next() {
        let line = std::str::from_utf8(raw).map_err(|_| err("path is not valid UTF-8"))?;

        if let Some(header) = line.strip_prefix("# ") {
            parse_header(header, &mut status.branch)?;
            continue;
        }

        let kind_char = line.chars().next().ok_or_else(|| err("empty entry"))?;
        match kind_char {
            '1' => {
                // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
                let f: Vec<&str> = line.splitn(9, ' ').collect();
                if f.len() != 9 {
                    return Err(err(format!("invalid '1' entry: {line}")));
                }
                let (index, worktree) = parse_xy(f[1])?;
                status.entries.push(StatusEntry {
                    kind: StatusKind::Change { index, worktree },
                    path: f[8].to_string(),
                });
            }
            '2' => {
                // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path> NUL <origPath>
                let f: Vec<&str> = line.splitn(10, ' ').collect();
                if f.len() != 10 {
                    return Err(err(format!("invalid '2' entry: {line}")));
                }
                let (index, worktree) = parse_xy(f[1])?;
                let mut score_chars = f[8].chars();
                let copy = match score_chars.next() {
                    Some('R') => false,
                    Some('C') => true,
                    _ => return Err(err(format!("invalid rename score: {}", f[8]))),
                };
                let score = score_chars
                    .as_str()
                    .parse::<u32>()
                    .map_err(|_| err(format!("invalid rename score: {}", f[8])))?;
                let original = tokens
                    .next()
                    .ok_or_else(|| err("rename entry is missing the original path"))?;
                let original_path = std::str::from_utf8(original)
                    .map_err(|_| err("path is not valid UTF-8"))?
                    .to_string();
                status.entries.push(StatusEntry {
                    kind: StatusKind::Rename {
                        index,
                        worktree,
                        copy,
                        score,
                        original_path,
                    },
                    path: f[9].to_string(),
                });
            }
            'u' => {
                // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
                let f: Vec<&str> = line.splitn(11, ' ').collect();
                if f.len() != 11 {
                    return Err(err(format!("invalid 'u' entry: {line}")));
                }
                status.entries.push(StatusEntry {
                    kind: StatusKind::Unmerged {
                        xy: f[1].to_string(),
                    },
                    path: f[10].to_string(),
                });
            }
            '?' | '!' => {
                let path = line
                    .strip_prefix(kind_char)
                    .and_then(|r| r.strip_prefix(' '))
                    .ok_or_else(|| err(format!("invalid entry: {line}")))?;
                status.entries.push(StatusEntry {
                    kind: if kind_char == '?' {
                        StatusKind::Untracked
                    } else {
                        StatusKind::Ignored
                    },
                    path: path.to_string(),
                });
            }
            other => return Err(err(format!("unknown entry type: {other}"))),
        }
    }

    Ok(status)
}

fn parse_header(header: &str, branch: &mut BranchInfo) -> Result<(), ParseError> {
    if let Some(v) = header.strip_prefix("branch.oid ") {
        branch.oid = Some(v.to_string());
    } else if let Some(v) = header.strip_prefix("branch.head ") {
        branch.head = Some(v.to_string());
    } else if let Some(v) = header.strip_prefix("branch.upstream ") {
        branch.upstream = Some(v.to_string());
    } else if let Some(v) = header.strip_prefix("branch.ab ") {
        // "+<ahead> -<behind>"
        let mut it = v.split(' ');
        let ahead = it.next().and_then(|s| s.strip_prefix('+'));
        let behind = it.next().and_then(|s| s.strip_prefix('-'));
        match (
            ahead.and_then(|s| s.parse().ok()),
            behind.and_then(|s| s.parse().ok()),
        ) {
            (Some(a), Some(b)) => branch.ab = Some((a, b)),
            _ => return Err(err(format!("invalid branch.ab: {v}"))),
        }
    }
    // 未知のヘッダ（stash 等）は無視する
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_and_entries_from_crafted_output() {
        let out = b"# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb dir/a b.txt\0\
2 R. N... 100644 100644 100644 aaa bbb R100 new name.txt\0old name.txt\0\
u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict file.txt\0\
? untracked.txt\0! ignored.log\0";
        let s = parse_status_v2(out).expect("parse");
        assert_eq!(s.branch.ab, Some((2, 1)));
        assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.entries.len(), 5);
        assert_eq!(s.entries[0].path, "dir/a b.txt");
        assert_eq!(
            s.entries[1].kind,
            StatusKind::Rename {
                index: StatusCode::Renamed,
                worktree: StatusCode::Unmodified,
                copy: false,
                score: 100,
                original_path: "old name.txt".to_string()
            }
        );
        assert_eq!(s.entries[2].path, "conflict file.txt");
        assert_eq!(s.unmerged().count(), 1);
    }

    #[test]
    fn rejects_non_utf8_paths() {
        let out = b"? \xff\xfe.txt\0";
        assert!(parse_status_v2(out).is_err());
    }

    #[test]
    fn rejects_truncated_entries() {
        assert!(parse_status_v2(b"1 .M N...\0").is_err());
        assert!(parse_status_v2(b"2 R. N... 1 1 1 a b R100 new\0").is_err());
    }
}
