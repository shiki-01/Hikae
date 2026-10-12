// 保存メモの提案の結果の分類と、提案の実行枠（設計 10.4・10.5）。Tauri に依存しない。
//
// - `SuggestFailure`: 提案が得られなかった理由。画面が文言を選ぶ（`Cancelled` は画面に出さない）
// - `failure_of`: 生成のエラーを理由に変える（接続先の種類で分ける）
// - `SuggestSlot`: 進行中の提案（同時に 1 件）。新しい要求は進行中の要求を取り消す。
//   取り消しは HTTP 要求の破棄までで、llama-server は止めない

use crate::error::LlmError;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio_util::sync::CancellationToken;

/// 提案に付ける注記（画面が短い文を選ぶ。内容は含まない）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SuggestNotice {
    /// 入力の中の機微に見える部分を伏せて作った
    SensitiveMasked,
    /// 機微なファイルの内容を使わなかった（名前と種類だけを渡した）
    SensitiveFileSkipped,
    /// 本文の行を、差分の本文を写していたため捨てた（内容を写している可能性）
    BodyDroppedCopy,
    /// 本文の行を、根拠が確認できなかったため捨てた
    BodyDroppedUngrounded,
    /// 本文の行を、機微に見える文字列を含むため捨てた（最も重い理由）
    BodyDroppedSensitive,
}

impl SuggestNotice {
    /// 本文の行を捨てた理由から注記を作る（`validate::BodyDrop`）
    pub fn from_body_drop(reason: crate::validate::BodyDrop) -> Self {
        use crate::validate::BodyDrop;
        match reason {
            BodyDrop::Ungrounded => SuggestNotice::BodyDroppedUngrounded,
            BodyDrop::Copy => SuggestNotice::BodyDroppedCopy,
            BodyDrop::Sensitive => SuggestNotice::BodyDroppedSensitive,
        }
    }
}

/// 提案が得られなかった理由（画面が文言を選ぶ）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SuggestFailure {
    /// 時間内に終わらなかった（入力の収集と生成の合計）
    Timeout,
    /// 変更の内容が無い（削除のみ・名前変更のみ・画像などだけ）。LLM は呼ばない（設計 10.4）。エラーではない
    NoContent,
    /// モデルの出力が検証で採用されなかった
    Invalid,
    /// AI の準備ができていない（提案の設定、モデルの未導入、Ollama に接続できない、接続先の指定）
    NotReady,
    /// そのほか（起動に失敗した、応答が壊れたなど）
    Unavailable,
    /// 利用者の操作、または新しい要求により取り消された（画面には出さない）
    Cancelled,
}

/// 生成のエラーを、提案の理由に変える。
/// `ollama` は接続先が Ollama か（内蔵では接続の失敗は「そのほか」、Ollama では「準備ができていない」）。
pub fn failure_of(e: LlmError, ollama: bool) -> SuggestFailure {
    match e {
        LlmError::Timeout => SuggestFailure::Timeout,
        LlmError::ModelNotInstalled
        | LlmError::UnknownModel
        | LlmError::InvalidModel
        | LlmError::CustomModelInvalid
        | LlmError::InvalidEndpoint => SuggestFailure::NotReady,
        LlmError::Network if ollama => SuggestFailure::NotReady,
        _ => SuggestFailure::Unavailable,
    }
}

/// 進行中の保存メモの提案（同時に 1 件）。
#[derive(Default)]
pub struct SuggestSlot {
    inner: Mutex<SlotInner>,
}

#[derive(Default)]
struct SlotInner {
    /// 要求の番号（新しい要求ごとに増える）
    seq: u64,
    /// 進行中の要求の取り消し用トークン
    current: Option<CancellationToken>,
}

impl SuggestSlot {
    fn lock(&self) -> MutexGuard<'_, SlotInner> {
        // 中で触るのは番号と取り消し用トークンだけのため、毒化していても続けて使ってよい
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 新しい要求を始める。進行中の要求は取り消す。戻り値は（番号, 取り消し用トークン）。
    /// 要求が終わったら `finish` に番号を渡す。
    pub fn begin(&self) -> (u64, CancellationToken) {
        let mut inner = self.lock();
        if let Some(previous) = inner.current.take() {
            previous.cancel();
        }
        inner.seq += 1;
        let token = CancellationToken::new();
        inner.current = Some(token.clone());
        (inner.seq, token)
    }

    /// 進行中の要求を取り消す（無ければ何もしない）
    pub fn cancel_current(&self) {
        if let Some(token) = self.lock().current.as_ref() {
            token.cancel();
        }
    }

    /// 要求が終わったとき、まだ自分が最新なら実行枠を空ける。新しい要求が始まっていれば何もしない。
    /// 戻り値は「自分が最新だったか」。最新でなければ、サーバーは新しい要求が使うため止めてはいけない
    pub fn finish(&self, seq: u64) -> bool {
        let mut inner = self.lock();
        if inner.seq == seq {
            inner.current = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_and_unknown_failures_map_to_their_reasons() {
        assert_eq!(
            failure_of(LlmError::Timeout, false),
            SuggestFailure::Timeout
        );
        for e in [
            LlmError::ModelNotInstalled,
            LlmError::UnknownModel,
            LlmError::InvalidModel,
            LlmError::InvalidEndpoint,
        ] {
            assert_eq!(failure_of(e, false), SuggestFailure::NotReady, "{e:?}");
        }
        for e in [
            LlmError::ServerNotFound,
            LlmError::ServerStartFailed,
            LlmError::ServerStartTimeout,
            LlmError::BadStatus,
            LlmError::BadResponse,
            LlmError::ResponseTooLarge,
            LlmError::Catalog,
            LlmError::Io,
        ] {
            assert_eq!(failure_of(e, false), SuggestFailure::Unavailable, "{e:?}");
        }
    }

    #[test]
    fn connection_failure_depends_on_the_backend() {
        // Ollama に接続できないのは「準備ができていない」。内蔵の接続の失敗は「そのほか」
        assert_eq!(
            failure_of(LlmError::Network, true),
            SuggestFailure::NotReady
        );
        assert_eq!(
            failure_of(LlmError::Network, false),
            SuggestFailure::Unavailable
        );
    }

    #[test]
    fn failure_names_are_kebab_case_on_the_wire() {
        let json = serde_json::to_string(&SuggestFailure::NotReady).unwrap();
        assert_eq!(json, "\"not-ready\"");
        let json = serde_json::to_string(&SuggestFailure::Cancelled).unwrap();
        assert_eq!(json, "\"cancelled\"");
        let json = serde_json::to_string(&SuggestFailure::NoContent).unwrap();
        assert_eq!(json, "\"no-content\"");
        // 注記の名前（本文を捨てた理由別）
        let json = serde_json::to_string(&SuggestNotice::BodyDroppedCopy).unwrap();
        assert_eq!(json, "\"body-dropped-copy\"");
        let json = serde_json::to_string(&SuggestNotice::BodyDroppedUngrounded).unwrap();
        assert_eq!(json, "\"body-dropped-ungrounded\"");
        let json = serde_json::to_string(&SuggestNotice::BodyDroppedSensitive).unwrap();
        assert_eq!(json, "\"body-dropped-sensitive\"");
    }

    #[test]
    fn begin_cancels_the_previous_request() {
        let slot = SuggestSlot::default();
        let (first, first_token) = slot.begin();
        assert!(!first_token.is_cancelled());
        let (second, second_token) = slot.begin();
        assert_ne!(first, second);
        // 新しい要求が来たら、進行中の要求を取り消す
        assert!(first_token.is_cancelled());
        assert!(!second_token.is_cancelled());
    }

    #[test]
    fn cancel_current_cancels_only_the_latest_request() {
        let slot = SuggestSlot::default();
        let (_, token) = slot.begin();
        slot.cancel_current();
        assert!(token.is_cancelled());
        // 進行中が無いときの取り消しは何もしない
        slot.finish(1);
        slot.cancel_current();
    }

    #[test]
    fn finish_clears_only_the_current_request() {
        let slot = SuggestSlot::default();
        let (first, first_token) = slot.begin();
        let (second, second_token) = slot.begin();
        assert!(first_token.is_cancelled());
        // 古い要求が終わっても、新しい要求の枠は空けない
        slot.finish(first);
        slot.cancel_current();
        assert!(second_token.is_cancelled());
        // 最新の要求が終わったら枠を空ける。以後の取り消しは、終わった要求には届かない
        let (third, third_token) = slot.begin();
        slot.finish(third);
        slot.cancel_current();
        assert!(!third_token.is_cancelled());
        assert_ne!(second, third);
    }
}
