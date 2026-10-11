// 保存メモの提案に使うモデルの指定（設計 10.5）。設定の値から決める純粋な判定。
// 設定型（core-store）には依存させず、値だけを受け取る。

use crate::custom::custom_model_id;
use std::path::{Path, PathBuf};

/// 提案に使うモデルの指定
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelChoice {
    /// 内蔵のカタログのモデル（ID）
    Catalog(String),
    /// 内蔵の、利用者が用意した GGUF（パス）
    Custom(PathBuf),
    /// Ollama のモデル名
    Ollama(String),
}

impl ModelChoice {
    /// 設定の値から決める。指定が空（未選択・パス未入力）なら `None`。
    /// - `ollama`: 実行方式が Ollama か。Ollama のときは入手元の設定に関係なく、モデル名を使う
    /// - `custom`: 内蔵で、入手元が利用者の GGUF か
    pub fn from_parts(ollama: bool, custom: bool, model: &str, custom_path: &str) -> Option<Self> {
        if ollama {
            return non_empty(model).map(|m| Self::Ollama(m.to_string()));
        }
        if custom {
            return non_empty(custom_path).map(|p| Self::Custom(PathBuf::from(p)));
        }
        non_empty(model).map(|id| Self::Catalog(id.to_string()))
    }

    /// キャッシュのキーとモデルの記録に使う ID。カスタムはパスの代わりに `custom:<ハッシュ>` を使う
    pub fn key(&self) -> String {
        match self {
            Self::Catalog(id) | Self::Ollama(id) => id.clone(),
            Self::Custom(path) => custom_model_id(path),
        }
    }

    /// 内蔵の GGUF（カスタム）のパス。それ以外は `None`
    pub fn custom_path(&self) -> Option<&Path> {
        match self {
            Self::Custom(path) => Some(path),
            _ => None,
        }
    }
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_uses_the_model_id() {
        let c = ModelChoice::from_parts(false, false, "gemma-4-e2b-it-q4_k_m", "C:\\x.gguf");
        assert_eq!(
            c,
            Some(ModelChoice::Catalog("gemma-4-e2b-it-q4_k_m".into()))
        );
        assert_eq!(c.unwrap().key(), "gemma-4-e2b-it-q4_k_m");
    }

    #[test]
    fn builtin_custom_uses_the_path_and_ignores_the_catalog_id() {
        let c = ModelChoice::from_parts(false, true, "gemma-4-e2b-it-q4_k_m", "C:\\m\\mine.gguf")
            .unwrap();
        assert_eq!(c, ModelChoice::Custom(PathBuf::from("C:\\m\\mine.gguf")));
        assert_eq!(c.custom_path(), Some(Path::new("C:\\m\\mine.gguf")));
        // キーはパスそのものではなく、ハッシュ由来の ID
        assert!(c.key().starts_with("custom:"));
        assert!(!c.key().contains("mine"));
    }

    #[test]
    fn ollama_ignores_the_source_setting() {
        // 入手元がカスタムでも、Ollama のときはモデル名を使う
        let c = ModelChoice::from_parts(true, true, "gemma3:4b", "C:\\m\\mine.gguf");
        assert_eq!(c, Some(ModelChoice::Ollama("gemma3:4b".into())));
        assert_eq!(c.unwrap().custom_path(), None);
    }

    #[test]
    fn empty_selection_is_none() {
        // 内蔵のカタログは ID が空なら未選択
        assert_eq!(
            ModelChoice::from_parts(false, false, "", "C:\\m.gguf"),
            None
        );
        // カスタムはパスが空なら未指定（ID が入っていても使わない）
        assert_eq!(ModelChoice::from_parts(false, true, "gemma", ""), None);
        // Ollama はモデル名が空なら未選択
        assert_eq!(ModelChoice::from_parts(true, false, "", ""), None);
    }

    #[test]
    fn catalog_key_is_the_id_so_existing_cache_entries_stay_valid() {
        let c = ModelChoice::Catalog("qwen3.5-2b-q4_k_m".into());
        assert_eq!(c.key(), "qwen3.5-2b-q4_k_m");
    }
}
