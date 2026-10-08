// 保存・取り込みの commit に付ける「どの PC で作ったか」の記録（トレーラー `Hikae-PC: <PC 名>`）。
//
// - メモ本文の末尾に空行を挟んで 1 行付ける（git のトレーラーと同じ形式）
// - 履歴・競合情報の取得時に解析し、`pc_name` として返す。メモ本文の表示にはトレーラーを含めない
// - PC 名は環境変数（COMPUTERNAME / HOSTNAME）、無ければホスト名のファイル・コマンドから得る。
//   取得できなければトレーラーを付けない。制御文字（改行を含む）は除き、長さを制限する

use core_git::GitRunner;
use core_safety::{RestorePoint, SafetyError, Signature};
use std::path::Path;
use std::sync::OnceLock;
use time::OffsetDateTime;

/// トレーラーのキー
const TRAILER_KEY: &str = "Hikae-PC";

/// PC 名の最大文字数
pub const MAX_PC_NAME_CHARS: usize = 64;

/// PC 名を整える。制御文字（改行・タブを含む）を除き、前後の空白を削り、長さを制限する。
/// 空になれば None。
pub fn sanitize_pc_name(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_PC_NAME_CHARS).collect())
}

/// この PC の名前。環境変数 → ホスト名ファイル → `hostname` コマンドの順に調べる。
/// 取得できなければ None（その場合トレーラーは付けない）。結果は最初の呼び出しで決め、使い回す。
pub fn local_pc_name() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(detect_pc_name).clone()
}

fn detect_pc_name() -> Option<String> {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Some(name) = std::env::var(key).ok().and_then(|v| sanitize_pc_name(&v)) {
            return Some(name);
        }
    }
    for path in ["/proc/sys/kernel/hostname", "/etc/hostname"] {
        if let Some(name) = std::fs::read_to_string(path)
            .ok()
            .and_then(|v| sanitize_pc_name(&v))
        {
            return Some(name);
        }
    }
    // macOS などは環境変数もファイルも無いことが多い。Windows は COMPUTERNAME が必ずあるため
    // ここへは来ない（コンソール窓を出さないよう、Windows では起動しない）
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("hostname")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if out.status.success() {
            return sanitize_pc_name(&String::from_utf8_lossy(&out.stdout));
        }
    }
    None
}

/// メモの末尾にトレーラーを付けた commit メッセージを返す。
/// PC 名が無い、またはメモが空のときは何も足さない（空メモは従来どおり commit 側で拒否される）。
pub(crate) fn with_trailer(message: &str, pc_name: Option<&str>) -> String {
    let Some(pc) = pc_name.and_then(sanitize_pc_name) else {
        return message.to_string();
    };
    if message.trim().is_empty() {
        return message.to_string();
    }
    format!("{}\n\n{TRAILER_KEY}: {pc}", message.trim_end())
}

/// commit メッセージを「メモ本文」と「PC 名」に分ける。トレーラーが無ければ全文とそのまま None。
/// 末尾の空白は除く。
pub(crate) fn split_trailer(message: &str) -> (String, Option<String>) {
    let trimmed = message.trim_end();
    let (before, last) = match trimmed.rfind('\n') {
        Some(i) => (&trimmed[..i], &trimmed[i + 1..]),
        None => ("", trimmed),
    };
    let prefix = format!("{TRAILER_KEY}:");
    match last.strip_prefix(prefix.as_str()) {
        Some(value) => (before.trim_end().to_string(), sanitize_pc_name(value)),
        None => (trimmed.to_string(), None),
    }
}

/// 状態変更操作が commit や復元点に付ける付帯情報（署名と PC 名）
#[derive(Clone, Copy)]
pub(crate) struct Meta<'a> {
    /// 自動保存（スナップショット）の作者
    pub signature: &'a Signature,
    /// この PC の名前（トレーラーに使う）
    pub pc_name: Option<&'a str>,
}

impl Meta<'_> {
    /// 復元点を作る（スナップショットの作者は `signature`）。
    pub fn restore_point(
        &self,
        runner: &GitRunner,
        repo: &Path,
        branch: &str,
        operation: &str,
        now: OffsetDateTime,
    ) -> Result<RestorePoint, SafetyError> {
        core_safety::create_restore_point_as(runner, repo, branch, operation, now, self.signature)
    }

    /// メモにトレーラーを付けた commit メッセージ
    pub fn message(&self, memo: &str) -> String {
        with_trailer(memo, self.pc_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_control_characters_and_limits_length() {
        assert_eq!(
            sanitize_pc_name("  DESKTOP-1 \n").as_deref(),
            Some("DESKTOP-1")
        );
        assert_eq!(sanitize_pc_name("a\r\nb\tc").as_deref(), Some("abc"));
        assert_eq!(sanitize_pc_name("\n \t"), None);
        assert_eq!(sanitize_pc_name(""), None);
        let long = "あ".repeat(200);
        assert_eq!(
            sanitize_pc_name(&long).map(|s| s.chars().count()),
            Some(MAX_PC_NAME_CHARS)
        );
    }

    #[test]
    fn trailer_is_appended_after_a_blank_line_and_can_be_split_off() {
        let full = with_trailer("報告書を更新", Some("研究室のPC"));
        assert_eq!(full, "報告書を更新\n\nHikae-PC: 研究室のPC");
        assert_eq!(
            split_trailer(&full),
            ("報告書を更新".to_string(), Some("研究室のPC".to_string()))
        );

        // 複数行のメモ
        let full = with_trailer("1行目\n2行目\n", Some("PC-A"));
        assert_eq!(full, "1行目\n2行目\n\nHikae-PC: PC-A");
        assert_eq!(
            split_trailer(&full),
            ("1行目\n2行目".to_string(), Some("PC-A".to_string()))
        );
    }

    #[test]
    fn messages_without_a_trailer_are_returned_as_they_are() {
        assert_eq!(split_trailer("memo\n"), ("memo".to_string(), None));
        assert_eq!(
            split_trailer("a\n\nSigned-off-by: x"),
            ("a\n\nSigned-off-by: x".to_string(), None)
        );
        // トレーラーを付けない条件
        assert_eq!(with_trailer("memo", None), "memo");
        assert_eq!(with_trailer("memo", Some("\n")), "memo");
        assert_eq!(with_trailer("  ", Some("PC")), "  ");
    }

    #[test]
    fn injected_newlines_in_the_pc_name_cannot_forge_other_lines() {
        let full = with_trailer("memo", Some("PC\nSigned-off-by: evil"));
        assert_eq!(full, "memo\n\nHikae-PC: PCSigned-off-by: evil");
        assert_eq!(full.lines().count(), 3);
    }

    #[test]
    fn a_blank_trailer_value_is_dropped_without_a_name() {
        assert_eq!(
            split_trailer("memo\n\nHikae-PC:   "),
            ("memo".to_string(), None)
        );
    }
}
