// ルールベースの保存メモ生成（設計書 10.3）。
//
// 純関数のみで構成する。文言はすべて呼び出し側が `MemoLabels` で渡し、
// このモジュールには表示言語に依存する文字列を置かない（既定値は英語）。

use serde::{Deserialize, Serialize};

/// 変更の種類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// メモ生成の入力となる 1 ファイル分の変更
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoChange {
    pub kind: MemoChangeKind,
    /// リポジトリ相対パス（`/` 区切り）。名前変更では新しいパス
    pub path: String,
    /// 名前変更の元のパス
    pub old_path: Option<String>,
    /// 変更量の目安（バイト数など）。混在時に「最も変更量の大きいファイル」を選ぶために使う
    pub weight: u64,
}

/// ファイルの種別（同種判定用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Image,
    Document,
    Other,
}

/// メモの文言テンプレート。呼び出し側が表示言語に合わせて渡す。
///
/// 使えるプレースホルダ:
/// - `{name}`: ファイル名（フォルダを除く）
/// - `{old}` / `{new}`: 名前変更の元と先のファイル名
/// - `{category}`: 種別名（`category_*` の値）
/// - `{count}`: 件数
#[derive(Debug, Clone)]
pub struct MemoLabels {
    pub added_one: String,
    pub modified_one: String,
    pub deleted_one: String,
    pub renamed_one: String,
    pub added_group: String,
    pub modified_group: String,
    pub deleted_group: String,
    pub renamed_group: String,
    /// 混在時。`{name}` は最も変更量の大きいファイル名、`{count}` は残りの件数
    pub mixed: String,
    pub category_image: String,
    pub category_document: String,
    pub category_other: String,
}

impl Default for MemoLabels {
    fn default() -> Self {
        MemoLabels {
            added_one: "Added {name}".to_string(),
            modified_one: "Updated {name}".to_string(),
            deleted_one: "Deleted {name}".to_string(),
            renamed_one: "Renamed {old} to {new}".to_string(),
            added_group: "Added {count} {category}".to_string(),
            modified_group: "Updated {count} {category}".to_string(),
            deleted_group: "Deleted {count} {category}".to_string(),
            renamed_group: "Renamed {count} {category}".to_string(),
            mixed: "{name} and {count} more".to_string(),
            category_image: "images".to_string(),
            category_document: "documents".to_string(),
            category_other: "files".to_string(),
        }
    }
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "heic", "tif", "tiff", "ico",
];

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "pdf", "txt", "md", "rtf", "odt", "ods", "odp",
    "csv",
];

/// パスの最後の要素（ファイル名）
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn category_of(path: &str) -> Category {
    let name = file_name(path);
    let ext = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => return Category::Other,
    };
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Category::Image
    } else if DOCUMENT_EXTENSIONS.contains(&ext.as_str()) {
        Category::Document
    } else {
        Category::Other
    }
}

fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (key, value) in pairs {
        s = s.replace(&format!("{{{key}}}"), value);
    }
    s
}

fn category_label(c: Category, labels: &MemoLabels) -> &str {
    match c {
        Category::Image => &labels.category_image,
        Category::Document => &labels.category_document,
        Category::Other => &labels.category_other,
    }
}

fn one_file_memo(change: &MemoChange, labels: &MemoLabels) -> String {
    let name = file_name(&change.path);
    match change.kind {
        MemoChangeKind::Added => fill(&labels.added_one, &[("name", name)]),
        MemoChangeKind::Modified => fill(&labels.modified_one, &[("name", name)]),
        MemoChangeKind::Deleted => fill(&labels.deleted_one, &[("name", name)]),
        MemoChangeKind::Renamed => {
            let old = change.old_path.as_deref().map(file_name).unwrap_or(name);
            fill(&labels.renamed_one, &[("old", old), ("new", name)])
        }
    }
}

/// 変更一覧から保存メモの案を作る。変更が空なら空文字列を返す。
///
/// - 1 ファイル: ファイル名つきの 1 文
/// - 2 件以上で、種別（画像・文書・その他）も変更の種類も同じ: 件数でまとめる
/// - それ以外: 最も変更量の大きいファイル名 + 残りの件数
pub fn suggest_memo(changes: &[MemoChange], labels: &MemoLabels) -> String {
    let Some(first) = changes.first() else {
        return String::new();
    };
    if changes.len() == 1 {
        return one_file_memo(first, labels);
    }

    let first_category = category_of(&first.path);
    let uniform = changes
        .iter()
        .all(|c| c.kind == first.kind && category_of(&c.path) == first_category);
    if uniform {
        let template = match first.kind {
            MemoChangeKind::Added => &labels.added_group,
            MemoChangeKind::Modified => &labels.modified_group,
            MemoChangeKind::Deleted => &labels.deleted_group,
            MemoChangeKind::Renamed => &labels.renamed_group,
        };
        return fill(
            template,
            &[
                ("category", category_label(first_category, labels)),
                ("count", &changes.len().to_string()),
            ],
        );
    }

    // 混在: 変更量が最大のもの（同値なら先に現れたもの）
    let mut heaviest = first;
    for c in &changes[1..] {
        if c.weight > heaviest.weight {
            heaviest = c;
        }
    }
    fill(
        &labels.mixed,
        &[
            ("name", file_name(&heaviest.path)),
            ("count", &(changes.len() - 1).to_string()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: MemoChangeKind, path: &str, weight: u64) -> MemoChange {
        MemoChange {
            kind,
            path: path.to_string(),
            old_path: None,
            weight,
        }
    }

    #[test]
    fn empty_changes_give_empty_memo() {
        assert_eq!(suggest_memo(&[], &MemoLabels::default()), "");
    }

    #[test]
    fn single_file_uses_file_name_without_folder() {
        let l = MemoLabels::default();
        let m = |k, p| suggest_memo(&[change(k, p, 1)], &l);
        assert_eq!(m(MemoChangeKind::Modified, "docs/a.docx"), "Updated a.docx");
        assert_eq!(m(MemoChangeKind::Added, "b.png"), "Added b.png");
        assert_eq!(m(MemoChangeKind::Deleted, "x/y/c.txt"), "Deleted c.txt");
    }

    #[test]
    fn rename_uses_old_and_new_names() {
        let c = MemoChange {
            kind: MemoChangeKind::Renamed,
            path: "dir/new.txt".to_string(),
            old_path: Some("dir/old.txt".to_string()),
            weight: 0,
        };
        assert_eq!(
            suggest_memo(&[c], &MemoLabels::default()),
            "Renamed old.txt to new.txt"
        );
    }

    #[test]
    fn same_category_and_kind_are_grouped() {
        let l = MemoLabels::default();
        let images = [
            change(MemoChangeKind::Added, "a.PNG", 1),
            change(MemoChangeKind::Added, "b.jpg", 1),
            change(MemoChangeKind::Added, "sub/c.webp", 1),
        ];
        assert_eq!(suggest_memo(&images, &l), "Added 3 images");
        let docs = [
            change(MemoChangeKind::Modified, "a.docx", 1),
            change(MemoChangeKind::Modified, "b.xlsx", 1),
        ];
        assert_eq!(suggest_memo(&docs, &l), "Updated 2 documents");
        let others = [
            change(MemoChangeKind::Deleted, "a.bin", 1),
            change(MemoChangeKind::Deleted, "Makefile", 1),
        ];
        assert_eq!(suggest_memo(&others, &l), "Deleted 2 files");
    }

    #[test]
    fn mixed_changes_use_heaviest_file_and_remaining_count() {
        let l = MemoLabels::default();
        let mixed = [
            change(MemoChangeKind::Added, "a.png", 10),
            change(MemoChangeKind::Modified, "report.docx", 500),
            change(MemoChangeKind::Deleted, "old.txt", 20),
        ];
        assert_eq!(suggest_memo(&mixed, &l), "report.docx and 2 more");
        // 同じ種別でも変更の種類が違えば混在扱い
        let kinds = [
            change(MemoChangeKind::Added, "a.png", 1),
            change(MemoChangeKind::Deleted, "b.png", 1),
        ];
        assert_eq!(suggest_memo(&kinds, &l), "a.png and 1 more");
    }

    #[test]
    fn labels_are_supplied_by_caller() {
        let labels = MemoLabels {
            modified_one: "<{name}>".to_string(),
            added_group: "{count}|{category}".to_string(),
            category_image: "IMG".to_string(),
            ..MemoLabels::default()
        };
        assert_eq!(
            suggest_memo(&[change(MemoChangeKind::Modified, "x/y.txt", 0)], &labels),
            "<y.txt>"
        );
        let two = [
            change(MemoChangeKind::Added, "1.gif", 0),
            change(MemoChangeKind::Added, "2.gif", 0),
        ];
        assert_eq!(suggest_memo(&two, &labels), "2|IMG");
    }

    #[test]
    fn hidden_dotfile_is_not_treated_as_extension() {
        assert_eq!(category_of(".png"), Category::Other);
        assert_eq!(category_of("dir/.gitignore"), Category::Other);
        assert_eq!(category_of("photo.jpeg"), Category::Image);
    }
}
