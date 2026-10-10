// 2 つの版の差分（設計書 3.3 S4）。段落は行単位（similar の Myers 法）、シートはセル座標の併合で比べる。
//
// - 段落: 変更の前後それぞれ 2 段落だけを文脈として残し、それ以外の変更のない段落は、
//   連続する数の省略マーカー（`ParagraphDiff::Omitted`）にまとめる
// - シート: 名前の完全一致で追加・削除を決める。改名は推測しない（削除と追加になる）。
//   同名のシートだけ、(行, 列) ごとにセルの追加・削除・変更を出す
// - 出力の順序は安定している（段落は文書の順、セルはシートの順 → 行 → 列）

use crate::xlsx::Sheet;
use similar::{capture_diff_slices_deadline, Algorithm, DiffOp};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// 変更の前後に残す文脈の段落数
const CONTEXT_LINES: usize = 2;

/// 段落の差分の計算に使う時間の上限。超えたら、そこまでの結果（変更が粗くなる）を使う。
/// 段落が多く、ほぼすべて違う文書で計算が長くなるのを防ぐ
const PARAGRAPH_DIFF_BUDGET: Duration = Duration::from_secs(3);

/// 段落の差分の 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParagraphDiff {
    /// 変更のない段落（文脈として残したもの）。番号は 1 始まり
    Context {
        old_no: usize,
        new_no: usize,
        text: String,
    },
    /// 追加された段落（新しい版の番号）
    Added { new_no: usize, text: String },
    /// 削除された段落（古い版の番号）
    Removed { old_no: usize, text: String },
    /// 省略された変更のない段落の件数
    Omitted { count: usize },
}

/// 段落の列の差分。変更のない段落は、文脈の範囲を除いて `Omitted` にまとめる
pub fn diff_paragraphs(old: &[String], new: &[String]) -> Vec<ParagraphDiff> {
    let deadline = Instant::now() + PARAGRAPH_DIFF_BUDGET;
    let ops = capture_diff_slices_deadline(Algorithm::Myers, old, new, Some(deadline));

    // 操作を 1 行ずつに展開する。Replace は削除の行の後に追加の行にする
    let mut lines = Vec::new();
    for op in ops {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for i in 0..len {
                    lines.push(Line::Equal(old_index + i, new_index + i));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for i in 0..old_len {
                    lines.push(Line::Removed(old_index + i));
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for i in 0..new_len {
                    lines.push(Line::Added(new_index + i));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                for i in 0..old_len {
                    lines.push(Line::Removed(old_index + i));
                }
                for i in 0..new_len {
                    lines.push(Line::Added(new_index + i));
                }
            }
        }
    }

    // 変更の行と、その前後 CONTEXT_LINES 行を残す
    let n = lines.len();
    let mut keep = vec![false; n];
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, Line::Equal(..)) {
            continue;
        }
        let start = i.saturating_sub(CONTEXT_LINES);
        let end = (i + CONTEXT_LINES + 1).min(n);
        for flag in &mut keep[start..end] {
            *flag = true;
        }
    }

    let mut out = Vec::new();
    let mut omitted = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if !keep[i] {
            // 残さない行は必ず変更のない行
            omitted += 1;
            continue;
        }
        if omitted > 0 {
            out.push(ParagraphDiff::Omitted { count: omitted });
            omitted = 0;
        }
        out.push(match *line {
            Line::Equal(o, n) => ParagraphDiff::Context {
                old_no: o + 1,
                new_no: n + 1,
                text: text_at(old, o),
            },
            Line::Removed(o) => ParagraphDiff::Removed {
                old_no: o + 1,
                text: text_at(old, o),
            },
            Line::Added(n) => ParagraphDiff::Added {
                new_no: n + 1,
                text: text_at(new, n),
            },
        });
    }
    if omitted > 0 {
        out.push(ParagraphDiff::Omitted { count: omitted });
    }
    out
}

/// 段落の差分の途中の行（インデックスは 0 始まり）
#[derive(Clone, Copy)]
enum Line {
    Equal(usize, usize),
    Removed(usize),
    Added(usize),
}

fn text_at(lines: &[String], index: usize) -> String {
    lines.get(index).cloned().unwrap_or_default()
}

/// セルの差分 1 件。`before` が None なら追加、`after` が None なら削除、両方あれば変更
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    /// シート名
    pub sheet: String,
    /// セル参照（A1 形式）
    pub cell: String,
    /// 行（0 始まり。`cell` と同じ位置。画面がグリッドに重ねるときに使う）
    pub row: u32,
    /// 列（0 始まり）
    pub col: u32,
    /// 古い版の値
    pub before: Option<String>,
    /// 新しい版の値
    pub after: Option<String>,
}

/// xlsx の差分。シートの追加・削除と、同名シートのセルの差分
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SheetsDiff {
    /// 新しい版にだけあるシート名（ブックの順）
    pub added_sheets: Vec<String>,
    /// 古い版にだけあるシート名（ブックの順）
    pub removed_sheets: Vec<String>,
    /// 同名のシートのセルの差分（新しい版のシート順 → 行 → 列）
    pub cells: Vec<CellChange>,
}

/// シートの列の差分。名前の一致で対応づけ、同名のシートだけセルを比べる
pub fn diff_sheets(old: &[Sheet], new: &[Sheet]) -> SheetsDiff {
    let old_by_name: BTreeMap<&str, &Sheet> = old.iter().map(|s| (s.name.as_str(), s)).collect();
    let new_by_name: BTreeMap<&str, &Sheet> = new.iter().map(|s| (s.name.as_str(), s)).collect();

    let added_sheets = new
        .iter()
        .filter(|s| !old_by_name.contains_key(s.name.as_str()))
        .map(|s| s.name.clone())
        .collect();
    let removed_sheets = old
        .iter()
        .filter(|s| !new_by_name.contains_key(s.name.as_str()))
        .map(|s| s.name.clone())
        .collect();

    let mut cells = Vec::new();
    for sheet in new {
        if let Some(before) = old_by_name.get(sheet.name.as_str()) {
            diff_cells(&sheet.name, &before.cells, &sheet.cells, &mut cells);
        }
    }
    SheetsDiff {
        added_sheets,
        removed_sheets,
        cells,
    }
}

/// 同名のシート 1 枚のセルを、(行, 列) の順に比べる
fn diff_cells(
    sheet: &str,
    old: &BTreeMap<(u32, u32), String>,
    new: &BTreeMap<(u32, u32), String>,
    out: &mut Vec<CellChange>,
) {
    let keys: BTreeSet<(u32, u32)> = old.keys().chain(new.keys()).copied().collect();
    for key in keys {
        let change = match (old.get(&key), new.get(&key)) {
            (Some(a), None) => Some((Some(a.clone()), None)),
            (None, Some(b)) => Some((None, Some(b.clone()))),
            (Some(a), Some(b)) if a != b => Some((Some(a.clone()), Some(b.clone()))),
            _ => None,
        };
        if let Some((before, after)) = change {
            out.push(CellChange {
                sheet: sheet.to_owned(),
                cell: cell_ref(key.0, key.1),
                row: key.0,
                col: key.1,
                before,
                after,
            });
        }
    }
}

/// (行, 列)（0 始まり）を A1 形式にする。列は A, …, Z, AA, … と続く
pub fn cell_ref(row: u32, col: u32) -> String {
    let mut letters = Vec::new();
    let mut n = u64::from(col) + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        // rem は 0〜25 なので、ASCII の大文字に必ず収まる
        letters.push(char::from(b'A' + u8::try_from(rem).unwrap_or(0)));
        n = (n - 1) / 26;
    }
    let mut text: String = letters.iter().rev().collect();
    text.push_str(&(u64::from(row) + 1).to_string());
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn cell_refs_follow_the_a1_form() {
        assert_eq!(cell_ref(0, 0), "A1");
        assert_eq!(cell_ref(2, 1), "B3");
        assert_eq!(cell_ref(0, 25), "Z1");
        assert_eq!(cell_ref(0, 26), "AA1");
        assert_eq!(cell_ref(0, 27), "AB1");
        assert_eq!(cell_ref(9, 701), "ZZ10");
        assert_eq!(cell_ref(1_048_575, 16_383), "XFD1048576");
    }

    #[test]
    fn identical_paragraphs_are_one_omitted_marker() {
        let a = strings(&["x", "y", "z"]);
        assert_eq!(
            diff_paragraphs(&a, &a),
            vec![ParagraphDiff::Omitted { count: 3 }]
        );
    }

    #[test]
    fn two_empty_documents_have_no_rows() {
        assert!(diff_paragraphs(&[], &[]).is_empty());
    }

    #[test]
    fn a_change_keeps_two_paragraphs_on_each_side() {
        let old: Vec<String> = (1..=10).map(|i| format!("p{i}")).collect();
        let mut new = old.clone();
        new[4] = "changed".to_owned();
        let rows = diff_paragraphs(&old, &new);
        let expected = vec![
            ParagraphDiff::Omitted { count: 2 },
            ParagraphDiff::Context {
                old_no: 3,
                new_no: 3,
                text: "p3".to_owned(),
            },
            ParagraphDiff::Context {
                old_no: 4,
                new_no: 4,
                text: "p4".to_owned(),
            },
            ParagraphDiff::Removed {
                old_no: 5,
                text: "p5".to_owned(),
            },
            ParagraphDiff::Added {
                new_no: 5,
                text: "changed".to_owned(),
            },
            ParagraphDiff::Context {
                old_no: 6,
                new_no: 6,
                text: "p6".to_owned(),
            },
            ParagraphDiff::Context {
                old_no: 7,
                new_no: 7,
                text: "p7".to_owned(),
            },
            ParagraphDiff::Omitted { count: 3 },
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn added_and_removed_paragraphs_are_reported_with_their_own_numbers() {
        let old = strings(&["a", "b", "c"]);
        let new = strings(&["a", "c", "d"]);
        let rows = diff_paragraphs(&old, &new);
        assert!(rows.contains(&ParagraphDiff::Removed {
            old_no: 2,
            text: "b".to_owned()
        }));
        assert!(rows.contains(&ParagraphDiff::Added {
            new_no: 3,
            text: "d".to_owned()
        }));
    }

    #[test]
    fn empty_paragraphs_are_kept_as_rows() {
        let old = strings(&["a", "b"]);
        let new = strings(&["a", "", "b"]);
        let rows = diff_paragraphs(&old, &new);
        assert!(rows.contains(&ParagraphDiff::Added {
            new_no: 2,
            text: String::new()
        }));
    }

    fn sheet(name: &str, cells: &[((u32, u32), &str)]) -> Sheet {
        Sheet {
            name: name.to_owned(),
            cells: cells.iter().map(|(k, v)| (*k, (*v).to_owned())).collect(),
        }
    }

    #[test]
    fn cell_changes_are_added_removed_and_changed_in_row_then_column_order() {
        let old = vec![sheet(
            "Sheet1",
            &[((2, 1), "120"), ((0, 0), "keep"), ((5, 0), "gone")],
        )];
        let new = vec![sheet(
            "Sheet1",
            &[((2, 1), "150"), ((0, 0), "keep"), ((0, 2), "new")],
        )];
        let diff = diff_sheets(&old, &new);
        assert!(diff.added_sheets.is_empty());
        assert!(diff.removed_sheets.is_empty());
        // 行 → 列の順（C1 は 1 行目、B3 は 3 行目）
        assert_eq!(
            diff.cells,
            vec![
                CellChange {
                    sheet: "Sheet1".to_owned(),
                    cell: "C1".to_owned(),
                    row: 0,
                    col: 2,
                    before: None,
                    after: Some("new".to_owned()),
                },
                CellChange {
                    sheet: "Sheet1".to_owned(),
                    cell: "B3".to_owned(),
                    row: 2,
                    col: 1,
                    before: Some("120".to_owned()),
                    after: Some("150".to_owned()),
                },
                CellChange {
                    sheet: "Sheet1".to_owned(),
                    cell: "A6".to_owned(),
                    row: 5,
                    col: 0,
                    before: Some("gone".to_owned()),
                    after: None,
                },
            ]
        );
    }

    #[test]
    fn sheets_are_matched_by_name_and_a_rename_is_a_removal_plus_an_addition() {
        let old = vec![sheet("Old", &[((0, 0), "1")]), sheet("Same", &[])];
        let new = vec![
            sheet("Same", &[((0, 0), "2")]),
            sheet("New", &[((0, 0), "3")]),
        ];
        let diff = diff_sheets(&old, &new);
        assert_eq!(diff.added_sheets, vec!["New".to_owned()]);
        assert_eq!(diff.removed_sheets, vec!["Old".to_owned()]);
        // 追加・削除のシートのセルは一覧に出さず、同名のシートだけ比べる
        assert_eq!(diff.cells.len(), 1);
        assert_eq!(diff.cells[0].sheet, "Same");
    }

    #[test]
    fn sheet_order_follows_the_new_workbook() {
        let old = vec![sheet("A", &[((0, 0), "1")]), sheet("B", &[((0, 0), "1")])];
        let new = vec![sheet("B", &[((0, 0), "2")]), sheet("A", &[((0, 0), "2")])];
        let names: Vec<String> = diff_sheets(&old, &new)
            .cells
            .into_iter()
            .map(|c| c.sheet)
            .collect();
        assert_eq!(names, vec!["B".to_owned(), "A".to_owned()]);
    }
}
