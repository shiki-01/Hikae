use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum XYStatus {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    UpdatedButUnmerged,
}

impl XYStatus {
    fn from_xy(x: char, y: char) -> Option<Self> {
        match (x, y) {
            (' ', ' ') => Some(XYStatus::Unmodified),
            ('M', _) | (_, 'M') => Some(XYStatus::Modified),
            ('A', _) | (_, 'A') => Some(XYStatus::Added),
            ('D', _) | (_, 'D') => Some(XYStatus::Deleted),
            ('R', _) | (_, 'R') => Some(XYStatus::Renamed),
            ('C', _) | (_, 'C') => Some(XYStatus::Copied),
            ('T', _) | (_, 'T') => Some(XYStatus::TypeChanged),
            ('U', _) | (_, 'U') => Some(XYStatus::UpdatedButUnmerged),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusKind {
    /// 通常の変更（1 タイプ）
    Change {
        xy: XYStatus,
        submodule_commit_changed: bool,
        mode_index: String,
        mode_worktree: String,
        object_index: String,
        object_worktree: String,
    },
    /// rename/copy（2 タイプ）。score は元ファイルから新ファイルへの類似度
    Rename {
        score: u32,
        original_path: String,
    },
    Copy {
        score: u32,
        original_path: String,
    },
    /// 競合（u タイプ）
    Unmerged {
        stage1_mode: String,
        stage2_mode: String,
        stage3_mode: String,
        stage1_object: String,
        stage2_object: String,
        stage3_object: String,
    },
    /// 未追跡（? タイプ）
    Untracked,
    /// 無視（! タイプ）
    Ignored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEntry {
    pub kind: StatusKind,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub oid: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ab: Option<(i32, i32)>, // (ahead, behind)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusV2 {
    pub branch: BranchInfo,
    pub entries: Vec<StatusEntry>,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "parse error: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

/// git status --porcelain=v2 -z --branch の出力を解析する
pub fn parse_status_v2(stdout: &[u8]) -> Result<StatusV2, ParseError> {
    let lines: Vec<&[u8]> = stdout.split(|&b| b == 0).collect();

    let mut branch = BranchInfo {
        oid: None,
        head: None,
        upstream: None,
        ab: None,
    };

    let mut entries = Vec::new();
    let mut line_idx = 0;

    // ブランチ情報を解析
    while line_idx < lines.len() {
        let line = lines[line_idx];

        if line.is_empty() {
            line_idx += 1;
            continue;
        }

        if !line.starts_with(b"#") {
            break;
        }

        let line_str = String::from_utf8_lossy(line);

        if let Some(v) = line_str.strip_prefix("# branch.oid ") {
            branch.oid = Some(v.to_string());
        } else if let Some(v) = line_str.strip_prefix("# branch.head ") {
            branch.head = Some(v.to_string());
        } else if let Some(v) = line_str.strip_prefix("# branch.upstream ") {
            branch.upstream = Some(v.to_string());
        } else if let Some(ab_str) = line_str.strip_prefix("# branch.ab ") {
            let parts: Vec<&str> = ab_str.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(ahead), Ok(behind)) = (
                    parts[0].trim_start_matches('+').parse::<i32>(),
                    parts[1].trim_start_matches('-').parse::<i32>(),
                ) {
                    branch.ab = Some((ahead, behind));
                }
            }
        }

        line_idx += 1;
    }

    // ステータスエントリを解析
    while line_idx < lines.len() {
        let line = lines[line_idx];

        if line.is_empty() {
            line_idx += 1;
            continue;
        }

        let line_str = String::from_utf8_lossy(line);

        if line_str.is_empty() {
            line_idx += 1;
            continue;
        }

        let first_char = line_str.chars().next().unwrap_or('\0');

        match first_char {
            '1' => {
                // 通常の変更
                // Format: 1 XY sub ... path
                let parts: Vec<&str> = line_str.split_whitespace().collect();
                if parts.len() < 9 {
                    return Err(ParseError {
                        message: format!("invalid '1' type entry: {}", line_str),
                    });
                }

                let xy = &line_str[2..4];
                let x = xy.chars().next().unwrap_or(' ');
                let y = xy.chars().nth(1).unwrap_or(' ');

                let xy_status = XYStatus::from_xy(x, y).ok_or_else(|| ParseError {
                    message: format!("invalid XY status: {}", xy),
                })?;

                let submodule_commit_changed = parts[3].contains('S');
                let mode_index = parts[4].to_string();
                let mode_worktree = parts[5].to_string();
                let object_index = parts[6].to_string();
                let object_worktree = parts[7].to_string();

                // パスはタブで区切られている
                let path = if let Some(tab_pos) = line.iter().position(|&b| b == b'\t') {
                    String::from_utf8_lossy(&line[tab_pos + 1..]).into_owned()
                } else {
                    return Err(ParseError {
                        message: "path not found in '1' entry".to_string(),
                    });
                };

                entries.push(StatusEntry {
                    kind: StatusKind::Change {
                        xy: xy_status,
                        submodule_commit_changed,
                        mode_index,
                        mode_worktree,
                        object_index,
                        object_worktree,
                    },
                    path,
                });
            }
            '2' => {
                // rename/copy
                // Format: 2 R/C XY ... oldpath<TAB>newpath
                let parts: Vec<&str> = line_str.split_whitespace().collect();
                if parts.len() < 5 {
                    return Err(ParseError {
                        message: format!("invalid '2' type entry: {}", line_str),
                    });
                }

                let sub_type = line_str.chars().nth(1).unwrap_or('\0');
                let xy = &line_str[3..5];
                let x = xy.chars().next().unwrap_or(' ');
                let y = xy.chars().nth(1).unwrap_or(' ');

                let _ = XYStatus::from_xy(x, y).ok_or_else(|| ParseError {
                    message: format!("invalid XY status in '2' entry: {}", xy),
                })?;

                let score = parts[4].parse::<u32>().map_err(|_| ParseError {
                    message: format!("invalid score: {}", parts[4]),
                })?;

                // パスを取得（タブで区切られている）
                let mut tabs = line.iter().enumerate().filter(|(_, &b)| b == b'\t');

                let first_tab = tabs.next().ok_or_else(|| ParseError {
                    message: "rename/copy entry missing first tab".to_string(),
                })?;

                let second_tab = tabs.next().ok_or_else(|| ParseError {
                    message: "rename/copy entry missing second tab".to_string(),
                })?;

                let original_path =
                    String::from_utf8_lossy(&line[first_tab.0 + 1..second_tab.0]).into_owned();
                let path = String::from_utf8_lossy(&line[second_tab.0 + 1..]).into_owned();

                let kind = match sub_type {
                    'R' => StatusKind::Rename {
                        score,
                        original_path,
                    },
                    'C' => StatusKind::Copy {
                        score,
                        original_path,
                    },
                    _ => {
                        return Err(ParseError {
                            message: format!("invalid '2' sub-type: {}", sub_type),
                        })
                    }
                };

                entries.push(StatusEntry { kind, path });
            }
            'u' => {
                // 競合（未マージ）
                let parts: Vec<&str> = line_str.split_whitespace().collect();
                if parts.len() < 11 {
                    return Err(ParseError {
                        message: format!("invalid 'u' type entry: {}", line_str),
                    });
                }

                let stage1_mode = parts[2].to_string();
                let stage2_mode = parts[3].to_string();
                let stage3_mode = parts[4].to_string();
                let stage1_object = parts[5].to_string();
                let stage2_object = parts[6].to_string();
                let stage3_object = parts[7].to_string();

                let path = if let Some(tab_pos) = line.iter().position(|&b| b == b'\t') {
                    String::from_utf8_lossy(&line[tab_pos + 1..]).into_owned()
                } else {
                    String::from_utf8_lossy(&line[parts[8].len()..]).into_owned()
                };

                entries.push(StatusEntry {
                    kind: StatusKind::Unmerged {
                        stage1_mode,
                        stage2_mode,
                        stage3_mode,
                        stage1_object,
                        stage2_object,
                        stage3_object,
                    },
                    path,
                });
            }
            '?' => {
                // 未追跡
                let path = if let Some(tab_pos) = line.iter().position(|&b| b == b'\t') {
                    String::from_utf8_lossy(&line[tab_pos + 1..]).into_owned()
                } else {
                    String::from_utf8_lossy(&line[2..]).into_owned()
                };

                entries.push(StatusEntry {
                    kind: StatusKind::Untracked,
                    path,
                });
            }
            '!' => {
                // 無視
                let path = if let Some(tab_pos) = line.iter().position(|&b| b == b'\t') {
                    String::from_utf8_lossy(&line[tab_pos + 1..]).into_owned()
                } else {
                    String::from_utf8_lossy(&line[2..]).into_owned()
                };

                entries.push(StatusEntry {
                    kind: StatusKind::Ignored,
                    path,
                });
            }
            _ => {
                return Err(ParseError {
                    message: format!("unknown entry type: {}", first_char),
                })
            }
        }

        line_idx += 1;
    }

    Ok(StatusV2 { branch, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_status() {
        let output = b"# branch.oid 1234567890abcdef\0# branch.head master\0";
        let result = parse_status_v2(output);
        assert!(result.is_ok());

        let status = result.unwrap();
        assert_eq!(status.branch.oid, Some("1234567890abcdef".to_string()));
        assert_eq!(status.branch.head, Some("master".to_string()));
        assert!(status.entries.is_empty());
    }

    #[test]
    fn test_xy_status_from_xy() {
        assert_eq!(XYStatus::from_xy('M', ' '), Some(XYStatus::Modified));
        assert_eq!(XYStatus::from_xy(' ', 'M'), Some(XYStatus::Modified));
        assert_eq!(XYStatus::from_xy('A', ' '), Some(XYStatus::Added));
        assert_eq!(XYStatus::from_xy('D', ' '), Some(XYStatus::Deleted));
        assert_eq!(XYStatus::from_xy(' ', ' '), Some(XYStatus::Unmodified));
    }
}
