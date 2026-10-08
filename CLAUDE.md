# Hikae

非エンジニア向けの GUI Git ツール（Windows / macOS デスクトップアプリ）。Git を「バックアップ」と「履歴管理」の用途に限定し、Git の用語を画面に一切出さずに全操作を完結させる。

- 詳細設計: `docs/design.md`（要件、フロー、Git コマンド対応、安全設計、設定、データモデル、ロードマップ）
- デザイン要件: `docs/design-requirements.md`（トークン名、コンポーネント、画面レイアウト）
- 実装前に該当章を必ず読むこと。設計と矛盾する実装が必要になった場合は、実装せずに理由を報告して判断を仰ぐ。実装に合わせた設計書の更新（追記・誤記修正）は Claude が行ってよい。CLAUDE.md も必要に応じて Claude が更新してよい

## 開発環境の前提

- Rust は 1.99.0 固定（`src-tauri/rust-toolchain.toml`）。パッケージマネージャは pnpm
- SvelteKit 3 のため、`src/lib` は `#lib` で参照する（`$lib` は使えない）。設定は `vite.config.ts` に書く
- Windows では tauri にリンクするテストが起動しないため、tauri 依存の crate（`app`）には単体テストを置かない。型生成は `cargo run -p hikae --bin export-bindings`

## 技術スタック（確定）

| 領域             | 採用                                                                                                                                                                                                                                 |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 基盤             | Tauri 2                                                                                                                                                                                                                              |
| UI               | SvelteKit（Svelte 5、adapter-static による SPA、SSR 無効）+ TypeScript                                                                                                                                                               |
| スタイル         | Master CSS（rc 版、`2.0.0-rc.88` に完全固定。正式版への追従はしない）。設定は CSS の `@theme`                                                                                                                                        |
| 状態管理         | Svelte 5 runes（UI 状態）+ TanStack Query の Svelte 版（バックエンド由来データ）                                                                                                                                                     |
| 型共有           | tauri-specta（Rust のコマンド定義から TS 型を生成）                                                                                                                                                                                  |
| Git              | 同梱の git CLI を `GitRunner` 経由で呼ぶ（libgit2 / isomorphic-git は使わない）。Windows は MinGit を `scripts/fetch-git-windows.mjs` で取得、macOS は `scripts/build-git-macos.sh`（未検証）。探索は `HIKAE_GIT_PATH` → 同梱 → PATH |
| ファイル監視     | notify + notify-debouncer-full                                                                                                                                                                                                       |
| 認証             | GitHub OAuth App の Device Flow、トークンは keyring（OS キーチェーン）                                                                                                                                                               |
| アプリ内データ   | SQLite（rusqlite）                                                                                                                                                                                                                   |
| ローカル LLM     | `llama-server` サイドカー（OpenAI 互換 API）、詳細設定で Ollama に切替                                                                                                                                                               |
| docx / xlsx 抽出 | zip + quick-xml、calamine                                                                                                                                                                                                            |
| ごみ箱への移動   | `trash` crate（`=5.2.9` に固定。`app` のみ。`core-ops` は `Trasher` trait だけを持つ）                                                                                                                                               |

## リポジトリ構成

```text
.
├── CLAUDE.md
├── docs/                 # 設計書（Claude も編集してよい。実装と食い違いが出たら更新する。設計の方針自体を変える場合は理由を報告する）
├── src/                  # SvelteKit フロントエンド
│   ├── lib/features/*    # projects, changes, history, compare, conflict, settings, ai, extensions
│   ├── lib/components/*  # デザイン要件 3章のコンポーネント
│   └── lib/bindings.ts   # tauri-specta の生成物（手で編集しない）
└── src-tauri/            # Cargo workspace
    ├── crates/core-git
    ├── crates/core-safety
    ├── crates/core-ops
    ├── crates/core-watch
    ├── crates/core-github
    ├── crates/core-preview
    ├── crates/core-llm
    ├── crates/core-store
    └── app/              # Tauri コマンド・イベント、操作キュー、スケジューラ
```

- `core-*` crate は Tauri に依存させない（単体テストと CLI での動作確認を可能にするため）
- 依存方向は `docs/design.md` 8.3 の表に従う。逆方向の依存を追加しない

## 安全上の不変条件（最優先。いかなる理由でも破らない）

1. 次の git 操作を実装・実行しない: `push --force` / `--force-with-lease` / `+refspec` / `--delete` / `--mirror`、`reset --hard`、`clean`、`branch -D`、`checkout -f`、`rebase`、`filter-branch`、`commit --amend`、`gc --prune=now`、`reflog expire`
2. git の呼び出しは必ず `core-git` の `GitRunner` を通す。`std::process::Command` で git を直接呼ばない。`GitRunner` は許可リストに一致しない引数を実行時に拒否する
3. 状態を変更する操作（保存、元に戻す、取り込み、ぶつかり解消、除外設定）は、実行前に `core-safety` で復元点を作る。復元点を作らずに作業フォルダやインデックスを変更するコードを書かない
4. 自動保存（スナップショット）は一時インデックス（`GIT_INDEX_FILE`）で作り、ユーザーの作業フォルダとインデックスを一切変更しない
5. `update-ref` は作成・更新・削除のすべてを `refs/<app>/` 名前空間に限る（値は完全な OID のみ）
6. 未追跡ファイルを復元不能な形で削除しない。利用者が明示的に「元に戻す（作成しない）」を選んだときに限り、直前のスナップショットに内容を残したうえで OS のごみ箱へ移す。追跡解除は `git rm --cached` のみ
7. 同一プロジェクトへの状態変更は `app` 層の直列キューで1件ずつ実行する
8. ユーザーのグローバル git 設定（`~/.gitconfig`）に書き込まない。必要な設定は `-c` またはリポジトリ単位で行う
9. トークンを平文でファイル・ログ・エラーメッセージに出さない

これらに関わる変更では、変更内容と不変条件を守っている根拠を報告に含めること。

## Git 呼び出しの既定値

- 出力は常に機械可読形式（`--porcelain=v2`、`-z`）で解析する。人間向け出力を正規表現で解析しない
- リポジトリ作成時に `core.autocrlf=false`、`core.precomposeUnicode=true` を設定する
- commit は `-c core.hooksPath=` を付けて実行する
- 署名は GitHub のユーザー名と noreply アドレスをリポジトリ単位で設定する

## UI の規約

- 画面上に Git 用語を出さない。用語は `docs/design.md` 2.1 の対応表に従う（例: commit → 保存、push → アップロード、conflict → 変更のぶつかり）
- 文言はすべて i18n キー経由（初期は日本語のみ）。コンポーネント内に日本語文字列を直書きしない
- 色・余白などは `docs/design-requirements.md` 2章のトークン名で Master CSS の `@theme` 変数（`src/lib/styles/tokens.css`）として定義し、コンポーネントでは値を直書きしない。値はデザイン確定前は仮でよい
- エラー表示は「何が起きたか」「データは無事か」「次の行動」の3要素（`docs/design.md` 5章）

## コーディング規約

- コードコメントは日本語で書く。ライブラリ名・技術用語はカタカナにせず英字のまま書く
- Rust: `cargo fmt`、`cargo clippy -- -D warnings` を通す。`unwrap()` は テスト以外で使わない
- TypeScript / Svelte: `prettier`、`eslint`、`svelte-check` を通す
- `core-git`・`core-safety`・`core-ops` には、一時ディレクトリに実リポジトリを作る統合テストを書く（モックで済ませない）

## 進め方

- 開発は `docs/design.md` 12章のフェーズ順に進める。現在のフェーズは Phase 1（MVP）の実装中。Phase 0 の結果は `docs/notes/phase0-report.md`。macOS での CI 実行は確認済み。Device Flow の実認証（ブラウザでの手動承認）は未確認
- 1つの作業単位ごとに commit する。commit メッセージは英語、Conventional Commits 形式
- 設計にない機能を追加しない。必要だと判断した場合は提案として報告する
- 判断に迷う点は推測で埋めず、選択肢と推奨を添えて質問する

## サブエージェント委任方針

機械的・定型的な作業（ファイル探索、チェック実行、テスト雛形作成、docs の下読みなど）は Agent ツールでサブエージェントに委任し、メインのコンテキストを消費しない。設計判断・アーキテクチャ選定・デバッグ方針の決定・安全上の不変条件に関わる判断は自分で行う。

委任するか迷った時点で、原則は委任する。委任しない場合はそれが例外であり、理由を一言添える。

利用可能なサブエージェント（`~/.claude/agents/`）:

| 名前              | 用途                                                                         |
| ----------------- | ---------------------------------------------------------------------------- |
| implementer       | 方針確定後の実装（新規ファイル・機能追加・リファクタリング）                 |
| bug-hunter        | エラー・テスト失敗・想定外の挙動の原因調査（修正はしない）                   |
| check-runner      | lint・型チェック・ビルドを実行し、失敗箇所だけを圧縮して報告（model: haiku） |
| codebase-explorer | 「どこに何があるか」の横断的な特定（読み取り専用、model: haiku）             |
| docs-reader       | docs/ の下読みと必要な節の要約（model: haiku）                               |
| test-scaffolder   | 既存の書き方に合わせたテスト雛形の作成（model: haiku）                       |

- サブエージェントの完了報告は鵜呑みにせず、差分とコマンド出力を自分で確認してから採用する。特に「テスト通過」「不変条件を充足」の記述は、`cargo clippy -- -D warnings` / `cargo test` / `pnpm check` / `pnpm lint` / `pnpm build` を自分で再実行して裏を取る
- 安全上の不変条件（直列キュー、復元点、許可リスト）の実装は、依頼文に該当する不変条件と検証方法を明記し、`unsafe` や no-op の疑似実装がないか読んで確認する
