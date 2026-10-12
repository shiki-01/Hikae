// 保存メモの提案の高水準 API（設計 10.4）。起動 → 生成 → 検証を 1 関数で行う。
// 失敗は `Err`、検証で弾かれた出力は `Ok(None)`。どちらもルールベースの文をそのまま使う前提。
//
// 保存操作は LLM を待たない。このサービスは呼び出し側（app 層）が非同期に呼ぶ。

use crate::catalog::Catalog;
use crate::client::{check_endpoint, Backend, ChatClient};
use crate::custom::validate_custom_gguf;
use crate::download::{installed, Installed};
use crate::error::LlmError;
use crate::prompt::{build_messages, ChangeInput, Message};
use crate::sidecar::{LlamaServer, LlamaServerOptions};
use crate::validate::{parse_memo, screen_memo, Memo};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// サービスの設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// モデルの保存先（`<models_dir>/<id>/<ファイル名>`）
    pub models_dir: PathBuf,
    /// 同梱された llama-server のパス（存在しなければ環境変数・PATH の順に探す）
    pub bundled_exe: Option<PathBuf>,
    /// 生成の接続先
    pub backend: Backend,
}

/// ローカル LLM による保存メモの提案
pub struct LlmService {
    backend: Backend,
    models_dir: PathBuf,
    catalog: Catalog,
    sidecar: LlamaServer,
    chat: ChatClient,
}

/// 生成の対象。内蔵のカタログの ID、利用者の GGUF のパス、Ollama の接続先とモデル名
enum Target<'a> {
    Catalog(&'a str),
    Custom(&'a Path),
    Ollama { base_url: &'a str, model: &'a str },
}

impl LlmService {
    /// 環境変数と PATH を読んで作る。
    pub fn new(config: ServiceConfig) -> Result<Self, LlmError> {
        let options = LlamaServerOptions::from_process_env(config.bundled_exe.clone());
        Self::with_options(config, options)
    }

    /// サイドカーの設定を指定して作る（時刻や待ち時間をテストで差し替えるため）。
    pub fn with_options(
        config: ServiceConfig,
        options: LlamaServerOptions,
    ) -> Result<Self, LlmError> {
        let catalog = Catalog::builtin().map_err(|_| LlmError::Catalog)?;
        let backend = validated_backend(config.backend)?;
        Ok(Self {
            backend,
            models_dir: config.models_dir,
            catalog,
            sidecar: LlamaServer::new(options)?,
            chat: ChatClient::new()?,
        })
    }

    /// 現在の接続先
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// 接続先を変える。Ollama の URL は検査し、正規化した値を保存する。
    /// ループバック以外のホストは拒否しない（呼び出し側は `check_endpoint` の `external_host` で警告する）。
    pub async fn set_backend(&mut self, backend: Backend) -> Result<(), LlmError> {
        let next = validated_backend(backend)?;
        if next != self.backend {
            // 接続先が変わったら、前の接続先の内蔵サーバーは止める
            self.sidecar.stop().await;
        }
        self.backend = next;
        Ok(())
    }

    /// 同梱のモデルカタログ
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// モデルの保存先
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// モデルが導入済みかどうか（導入状態は `download::installed` と同じ判定）。
    pub fn is_model_ready(&self, model_id: &str) -> Result<bool, LlmError> {
        let entry = self.catalog.get(model_id).ok_or(LlmError::UnknownModel)?;
        Ok(installed(&self.models_dir, entry).map_err(|_| LlmError::Io)? == Installed::Ready)
    }

    /// 内蔵の llama-server を止める（アプリの終了時など）。
    pub async fn stop(&self) {
        self.sidecar.stop().await;
    }

    /// 保存メモの提案を生成する（要約と、`body` が真なら本文）。
    ///
    /// - `model_ref`: 内蔵の接続先ではカタログの ID、Ollama ではモデル名
    /// - `body`: 本文も求めるか（偽なら要約だけを求め、本文は返さない）
    /// - `timeout`: 生成の上限。内容に触れていない出力への書き直しも、この中で行う（合計）。
    ///   起動待ち（内蔵）は含まず、別に上限がある（`sidecar`）。呼び出し側が全体の上限を管理し、残りの時間を渡す
    /// - 変更ファイルが空なら、サーバーを起動せず `Ok(None)` を返す
    /// - 検証で弾かれた出力は `Ok(None)`。接続・起動・タイムアウトなどは `Err`
    /// - 内蔵の生成で応答が壊れた・接続が切れたときだけ子プロセスを止める（次回に起動し直す）。
    ///   タイムアウトや呼び出しの取り消しでは止めない（止めると次回の起動が遅くなるため）
    pub async fn suggest(
        &self,
        input: &ChangeInput,
        model_ref: &str,
        timeout: Duration,
        body: bool,
    ) -> Result<Option<Memo>, LlmError> {
        if input.files.is_empty() {
            return Ok(None);
        }
        if !is_valid_model_ref(model_ref) {
            return Err(LlmError::InvalidModel);
        }
        let target = match &self.backend {
            Backend::Builtin => Target::Catalog(model_ref),
            Backend::Ollama { base_url } => Target::Ollama {
                base_url,
                model: model_ref,
            },
        };
        self.suggest_with(input, &target, timeout, body).await
    }

    /// 利用者が用意した GGUF（カスタム）で保存メモの提案を 1 文生成する（内蔵の接続先だけ）。
    ///
    /// - 先に `validate_custom_gguf` で検査する（読み取りのみ。拒否は `CustomModelInvalid`）
    /// - ファイルはコピー・移動しない。`-m` に渡すパスを内蔵のサーバーに与えるだけ
    /// - 停止・再起動の扱いは `suggest` と同じ（応答が壊れたときだけ止める）
    /// - 変更ファイルが空なら、サーバーを起動せず `Ok(None)` を返す
    pub async fn suggest_custom(
        &self,
        input: &ChangeInput,
        gguf_path: &Path,
        timeout: Duration,
        body: bool,
    ) -> Result<Option<Memo>, LlmError> {
        if input.files.is_empty() {
            return Ok(None);
        }
        if self.backend != Backend::Builtin {
            return Err(LlmError::InvalidModel);
        }
        validate_custom_gguf(gguf_path).map_err(|_| LlmError::CustomModelInvalid)?;
        self.suggest_with(input, &Target::Custom(gguf_path), timeout, body)
            .await
    }

    /// 1 回生成して検証する。出力が件数だけで差分の内容に触れていなければ、残りの時間の中で 1 回だけ書き直させる。
    ///
    /// - 1 回目が検証で弾かれたとき（`None`）は、書き直さない
    /// - 書き直しが失敗・時間切れ・検証で不採用のときは、1 回目の結果を返す（変更の内容は無事のまま）
    /// - 書き直しの結果も件数だけなら、1 回目の結果を返す
    /// - 要約だけを求めるとき（`body` が偽）は、出力の本文を捨てる
    async fn suggest_with(
        &self,
        input: &ChangeInput,
        target: &Target<'_>,
        timeout: Duration,
        body: bool,
    ) -> Result<Option<Memo>, LlmError> {
        let started = Instant::now();
        let raw = self
            .generate(&build_messages(input, body), target, timeout)
            .await?;
        let Some(first) = finish(&raw, input, body) else {
            return Ok(None);
        };
        if !needs_content_retry(input, &first.title) {
            return Ok(Some(first));
        }
        let left = timeout.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Ok(Some(first));
        }
        let second = match self
            .generate(&content_retry_messages(input, body), target, left)
            .await
        {
            Ok(raw) => finish(&raw, input, body),
            Err(_) => None,
        };
        match second {
            Some(memo) if !needs_content_retry(input, &memo.title) => Ok(Some(memo)),
            _ => Ok(Some(first)),
        }
    }

    /// 接続先に合わせて 1 回生成する。戻り値は検証前の生の出力。
    async fn generate(
        &self,
        messages: &[Message],
        target: &Target<'_>,
        timeout: Duration,
    ) -> Result<String, LlmError> {
        match target {
            Target::Catalog(model_ref) => {
                let entry = self.catalog.get(model_ref).ok_or(LlmError::UnknownModel)?;
                if installed(&self.models_dir, entry).map_err(|_| LlmError::Io)? != Installed::Ready
                {
                    return Err(LlmError::ModelNotInstalled);
                }
                let path = self.models_dir.join(&entry.id).join(&entry.file_name);
                self.generate_builtin(&path, &entry.file_name, messages, timeout)
                    .await
            }
            Target::Custom(path) => {
                self.generate_builtin(path, CUSTOM_MODEL_NAME, messages, timeout)
                    .await
            }
            Target::Ollama { base_url, model } => {
                let endpoint = check_endpoint(base_url)?;
                self.chat
                    .chat(&endpoint.base_url, model, messages, timeout)
                    .await
            }
        }
    }

    /// 内蔵の llama-server を起動して 1 回生成する。応答が壊れたときだけ止める（`breaks_server`）。
    async fn generate_builtin(
        &self,
        path: &Path,
        model_name: &str,
        messages: &[Message],
        timeout: Duration,
    ) -> Result<String, LlmError> {
        let base_url = self.sidecar.ensure_started(path).await?;
        let result = self
            .chat
            .chat(&base_url, model_name, messages, timeout)
            .await;
        if let Err(e) = &result {
            if breaks_server(e) {
                self.sidecar.stop().await;
            }
        }
        result
    }
}

/// カスタム GGUF のとき、接続先へ渡すモデル名。パスは渡さない（ログや要求の内容に残さないため）
const CUSTOM_MODEL_NAME: &str = "custom-gguf";

/// 内容に触れていない出力への書き直しを頼む指示（ユーザーメッセージの末尾に足す。書き直しは 1 回だけ）
const RETRY_NOTE: &str = "直前の出力は、変更の件数だけを書いており、追加・変更された内容に触れていません。差分の本文にある具体的な語（追加された文の語など）を使い、何がどう変わったかを 1 文で書き直してください。「N行追加」「N件更新」だけの文は書かないでください。";

/// 件数の単位（数字の後ろに続くとき、件数の表現とみなす）
const COUNT_UNITS: [&str; 8] = ["行", "件", "段落", "セル", "シート", "箇所", "個", "枚"];

/// 差分の本文の語とみなす最短の長さ（文字数）。これ未満の並びは語として数えない
const MIN_WORD_CHARS: usize = 3;

/// 出力が「数だけ」の文で、差分の本文の語に触れていないか（再生成の判定。純関数）。
///
/// 次の 3 つがそろったときだけ `true` にする。
/// 1. 出力に件数の表現（数字の後ろに「行」「件」「段落」などが続く。例: 「2行追加」「1 件更新」）がある
/// 2. 差分の本文に、ひらがな・カタカナ・漢字・英字の 3 文字以上のかたまり（語）がある
/// 3. その語のどれも、出力に含まれない
///
/// 本文が無いとき（画像・削除のみ・本文を渡さない設定）は再生成しない。
pub fn needs_content_retry(input: &ChangeInput, text: &str) -> bool {
    if !has_count_phrase(text) {
        return false;
    }
    let words = body_words(input);
    if words.is_empty() {
        return false;
    }
    !words.iter().any(|word| text.contains(word.as_str()))
}

/// 内容に触れていない出力への書き直しを頼むメッセージ。システムの指示は変えず、ユーザーの末尾に `RETRY_NOTE` を足す。
pub fn content_retry_messages(input: &ChangeInput, body: bool) -> Vec<Message> {
    let mut messages = build_messages(input, body);
    let user = messages[1].content.trim_end().to_string();
    messages[1].content = format!("{user}\n{RETRY_NOTE}\n");
    messages
}

/// 数字（半角・全角）の後ろに件数の単位が続く表現があるか（空白は許す）
fn has_count_phrase(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if !is_digit(*c) {
            continue;
        }
        let rest: String = chars[i..]
            .iter()
            .skip_while(|c| is_digit(**c))
            .skip_while(|c| c.is_whitespace())
            .take(4)
            .collect();
        if COUNT_UNITS.iter().any(|unit| rest.starts_with(unit)) {
            return true;
        }
    }
    false
}

/// 半角・全角の数字
fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c)
}

/// 差分の本文（全ファイル）から、語（3 文字以上のかたまり）を取り出す
fn body_words(input: &ChangeInput) -> Vec<String> {
    input
        .files
        .iter()
        .filter_map(|file| file.diff_text.as_deref())
        .flat_map(|text| text.split(|c: char| !is_word_char(c)))
        .filter(|word| word.chars().count() >= MIN_WORD_CHARS)
        .map(str::to_string)
        .collect()
}

/// 語を作る文字（ひらがな・カタカナ・漢字・英字）。数字と記号は含めない
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(
            c,
            '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
        )
}

/// 生の出力を解析し、入力と照らして検証する（要約だけを求めたときは本文を捨てる）。
fn finish(raw: &str, input: &ChangeInput, body: bool) -> Option<Memo> {
    let mut memo = parse_memo(raw)?;
    if !body {
        memo.body.clear();
        memo.body_dropped = false;
    }
    screen_memo(memo, input)
}

/// 接続先を検査して正規化する。内蔵はそのまま通す。
fn validated_backend(backend: Backend) -> Result<Backend, LlmError> {
    match backend {
        Backend::Builtin => Ok(Backend::Builtin),
        Backend::Ollama { base_url } => Ok(Backend::Ollama {
            base_url: check_endpoint(&base_url)?.base_url,
        }),
    }
}

/// 生成の失敗が、サーバーを止めて起動し直す理由になるか。
/// タイムアウトは含めない（サーバーは正常で、処理が長いだけのことが多いため）。
fn breaks_server(e: &LlmError) -> bool {
    matches!(
        e,
        LlmError::Network
            | LlmError::BadStatus
            | LlmError::BadResponse
            | LlmError::ResponseTooLarge
    )
}

/// モデル指定の文字列として妥当か（空、制御文字、極端な長さを拒否する）。
fn is_valid_model_ref(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{ChangeKind, ChangedFile, Role};

    fn input_with(diffs: &[Option<&str>]) -> ChangeInput {
        ChangeInput {
            files: diffs
                .iter()
                .enumerate()
                .map(|(i, d)| ChangedFile {
                    kind: ChangeKind::Modified,
                    name: format!("f{i}.txt"),
                    old_name: None,
                    amount: Some("行 +2 / -0".into()),
                    diff_text: d.map(str::to_string),
                    sensitive: false,
                })
                .collect(),
        }
    }

    #[test]
    fn only_broken_connections_stop_the_server() {
        // 応答が壊れた・接続が切れたときは止めて起動し直す
        for e in [
            LlmError::Network,
            LlmError::BadStatus,
            LlmError::BadResponse,
            LlmError::ResponseTooLarge,
        ] {
            assert!(breaks_server(&e), "{e:?}");
        }
        // タイムアウトや、そのほかの失敗では止めない（処理が長いだけのことが多く、起動し直すと遅くなる）
        for e in [
            LlmError::Timeout,
            LlmError::ModelNotInstalled,
            LlmError::ServerNotFound,
        ] {
            assert!(!breaks_server(&e), "{e:?}");
        }
    }

    #[test]
    fn count_only_output_with_unmentioned_body_needs_a_retry() {
        // 観察された症状: 件数と名前だけで、追加された文に触れていない
        let input = input_with(&[
            Some("+ 明日の会議は10時から。\n+ 資料は3部印刷する。\n"),
            Some("+ 牛乳と卵を買う\n"),
        ]);
        assert!(needs_content_retry(
            &input,
            "test.txtに2行追加し、新規 テキスト ドキュメント.txtに1行追加した。"
        ));
        // 語に触れていれば書き直さない（件数があっても）
        assert!(!needs_content_retry(
            &input,
            "test.txtに会議の時間を10時からと資料の印刷を追記し、牛乳と卵の購入を追加した。"
        ));
        // 件数の表現がなければ（ファイル名と操作だけの文は）ここでは判定しない
        assert!(!needs_content_retry(
            &input,
            "test.txt を更新し、新規 テキスト ドキュメント.txt を追加した。"
        ));
    }

    #[test]
    fn a_short_body_word_quoted_back_is_enough() {
        // 意味の薄い内容（「テスト」）は、その語を引用していれば触れたことになる
        let input = input_with(&[Some("+ テスト\n+ aaa\n")]);
        assert!(!needs_content_retry(
            &input,
            "test.txtに「テスト」を2行追加した。"
        ));
        assert!(needs_content_retry(&input, "test.txtに2行追加した。"));
    }

    #[test]
    fn no_retry_without_body_words() {
        // 本文が無い（画像・削除のみ・本文を渡さない設定）、または語が 3 文字未満のときは書き直さない
        let no_body = input_with(&[None, Some("")]);
        assert!(!needs_content_retry(&no_body, "2件を削除した。"));
        let short_words = input_with(&[Some("+ ab\n+ 1\n")]);
        assert!(!needs_content_retry(&short_words, "2行追加した。"));
    }

    #[test]
    fn count_phrase_detection() {
        assert!(has_count_phrase("2行追加した"));
        assert!(has_count_phrase("更新 3 件"));
        assert!(has_count_phrase("段落２件を修正"));
        assert!(has_count_phrase("セル 4 件"));
        // 数字の後ろが件数の単位でないものは、件数とみなさない
        assert!(!has_count_phrase("10時から開始する"));
        assert!(!has_count_phrase("3部印刷する"));
        assert!(!has_count_phrase("件数を減らした"));
    }

    #[test]
    fn body_words_are_three_char_runs_of_kana_kanji_or_latin() {
        let input = input_with(&[Some(
            "+ 明日の会議は10時から。\n- ab-cd\n+ ABC-123 テスト\n",
        )]);
        let words = body_words(&input);
        assert!(words.contains(&"明日の会議は".to_string()), "{words:?}");
        assert!(words.contains(&"時から".to_string()), "{words:?}");
        assert!(words.contains(&"ABC".to_string()), "{words:?}");
        assert!(words.contains(&"テスト".to_string()), "{words:?}");
        // 2 文字以下の英字と、数字だけの並びは語にしない
        assert!(!words.contains(&"ab".to_string()));
        assert!(!words.iter().any(|w| w.chars().all(|c| c.is_ascii_digit())));
    }

    #[test]
    fn retry_messages_keep_the_system_prompt_and_add_the_note_once() {
        let input = input_with(&[Some("+ 牛乳を買う\n")]);
        let first = build_messages(&input, false);
        let retry = content_retry_messages(&input, false);
        assert_eq!(retry.len(), 2);
        assert_eq!(retry[0], first[0]);
        assert_eq!(retry[0].role, Role::System);
        assert!(retry[1].content.starts_with(first[1].content.trim_end()));
        assert!(retry[1].content.ends_with(&format!("{RETRY_NOTE}\n")));
        assert_eq!(retry[1].content.matches(RETRY_NOTE).count(), 1);
    }
}

#[cfg(test)]
mod finish_tests {
    use super::*;
    use crate::prompt::{ChangeKind, ChangedFile};

    fn input_with_diff(diff: &str) -> ChangeInput {
        ChangeInput {
            files: vec![ChangedFile {
                kind: ChangeKind::Modified,
                name: "契約書.docx".into(),
                old_name: None,
                amount: Some("段落 +1 / -0".into()),
                diff_text: Some(diff.into()),
                sensitive: false,
            }],
        }
    }

    #[test]
    fn invalid_body_lines_are_dropped_and_the_summary_is_kept() {
        let input = input_with_diff("+ 支払期限は翌々月の十五日とする");
        let raw =
            "契約書の支払期限を変えた。\n\n・支払期限を翌々月払いに変更\n・commit の内容を書いた";
        let memo = finish(raw, &input, true).expect("要約は採る");
        assert_eq!(memo.title, "契約書の支払期限を変えた。");
        assert_eq!(memo.body, vec!["支払期限を翌々月払いに変更"]);
        assert!(memo.body_dropped);
    }

    #[test]
    fn a_sensitive_summary_makes_the_whole_output_invalid() {
        let input = input_with_diff("+ 設定");
        let raw = "接続先の password=hunter2 を変えた。\n\n・設定を変更";
        assert_eq!(finish(raw, &input, true), None);
    }

    #[test]
    fn summary_only_mode_drops_the_body_even_if_the_model_wrote_one() {
        let input = input_with_diff("+ 支払期限を翌々月払いに変える");
        let raw = "契約書の支払期限を変えた。\n\n・支払期限を翌々月払いに変更";
        let memo = finish(raw, &input, false).expect("要約");
        assert!(memo.body.is_empty());
        assert!(!memo.body_dropped);
    }
}
