# Phase 0 報告

作成日: 2026-10-08

Phase 0（技術検証）の結果です。Phase 0 の時点では、環境は Windows 11 のみで、macOS と GitHub Actions 上の CI は未検証でした。その後の確認と、Phase 1 の実装中に見つけた不具合は、末尾の「8. Phase 0 以降の確認」に追記しています（1〜7 章は Phase 0 の時点の記録で、現状と異なる箇所には注記を付けました）。

## 1. 構成

- フロントエンド: SvelteKit 3、Svelte 5、adapter-static の SPA（`ssr = false`）、TypeScript 6.0.3、Vite 8
- Master CSS: `@master/css` と `@master/css.vite` を `2.0.0-rc.88` に完全固定。トークンは `src/lib/styles/tokens.css`（light/dark、Phase 0 の時点では値は仮。現在は Figma の確定値に差し替え済み。`docs/design-requirements.md` 2.1）
- Rust: `src-tauri` の Cargo workspace に `core-*` 8 crate と `app`。tauri-specta が `src/lib/bindings.ts` を生成
- ツールチェーン: Rust 1.99.0（`src-tauri/rust-toolchain.toml` で固定）、パッケージマネージャは pnpm

## 2. 検証項目の結果

| # | 項目 | 結果 | 根拠 |
| --- | --- | --- | --- |
| 1 | core-git の許可リスト | 通った | サブコマンドごとの default deny 方式。拒否テストは約 100 ケース（省略形、結合形、`--` 後ろの `+refspec` を含む）。実リポジトリでの実行テストと status パーサのテストあり |
| 2 | core-safety の一時インデックス | 通った | 作業フォルダの内容と mtime、`.git/index` のバイト列、`status` が不変。tree の中身を検証。`refs/hikae/*` は push されない |
| 3 | core-ops の 2 clone 統合テスト | 通った（14 件） | 保存、アップロード、取り込み、競合、「この PC の版」／「クラウドの版」の 2 択解消。削除と変更の競合、`merge --abort`、`pre-merge` バックアップ、公開履歴が書き換わらないことを確認 |
| 4 | core-github の Device Flow | 実装済み、実認証は未確認 | モックテスト 21 件。keyring の保存・読み出し・削除は `--ignored` の 2 件で確認。手順は `src-tauri/crates/core-github/README.md` |
| 5 | git 同梱の調査 | 完了 | `docs/notes/git-bundling.md` |

あわせて、`pnpm tauri dev` でウィンドウが起動し、`pnpm tauri build --debug`（MSI / NSIS 生成）が通ることを確認しました。

## 3. 設計書へ反映した発見

1. Rust 1.99 以上が必要。手元の 1.89 では specta rc.25 がコンパイルできず、tauri 2.12 も 1.90 以上を要求する
2. Master CSS rc の設定は `master.css.ts` ではなく CSS の `@theme`。トークン名は名前空間付き（例: `--color-bg`）。使われた変数だけが CSS に出力される。SvelteKit 3 では `svelte.config.js` が廃止され、`$lib` が `#lib` に変わった
3. 許可リストと設計書のずれ
   - `rm` は `--cached` 必須。「削除を受け入れる」は `rm --cached` とファイル削除で実装
   - `update-ref` は作成・更新・削除のすべてを `refs/<app>/` 配下に限定し、値は完全な OID のみ
   - `fetch --prune` は許可
4. `GitRunner::run` は終了コードが非ゼロでも `Ok` を返す。`run_ok` と `run_allow_codes` を追加
5. Windows では、tauri にリンクするテスト実行ファイルが起動しない（`STATUS_ENTRYPOINT_NOT_FOUND`）。型生成は別 bin の `export-bindings` に分けた
6. `status --porcelain=v2 -z` は NUL 区切りで、パスは空白区切りフィールドの末尾に置かれる。rename の元パスだけが次のトークンになる
7. メモと別名コピーの文言は、呼び出し側が `Labels` で渡す設計（コアに日本語を置かない）

## 4. 実装中に見つけて直した不具合

- core-git: `push` の `+refspec` を `--` の後ろに置くと検査をすり抜けた。`update-ref -d` を 2 回指定すると最後の ref しか検査されなかった。全ゼロ OID で `-d` なしに ref を削除できた。status パーサが非 `-z` 形式を前提としていた
- core-ops: `init_project` / `clone_project` がディレクトリ作成前に cwd にして起動していた。競合検出のフィールド位置がずれていた。別名コピーが元のフォルダに作られず、既存ファイルを上書きした
- core-github: keyring の削除を空パスワードで代用していた。`DeviceCode` の Debug に `device_code` が出ていた

## 5. 未実施・未確認

Phase 0 の時点の項目と、2026-10-08 時点の現状です。

- CI（`.github/workflows/ci.yml`）: 当時は一度も実行していなかった。**現在は実行済みで、Windows・macOS とも成功している**（8.1）。macOS の実機での確認は未実施
- ウィンドウ内から Rust コマンドを呼ぶ処理（IPC）: 当時は未実装。**現在は実装済み**（39 個のコマンドを登録し、画面から呼ぶ）。ウィンドウ上での通し操作の確認記録は、リポジトリからは確認できない
- Device Flow の実認証（手動確認が必要）: **未確認だったが、その後に利用者が確認済み（2026-10-09）**。実際の GitHub の応答に合わせた修正を 1 件入れた（8.2 の 5）
- macOS の git 同梱サイズ、署名・公証: **現在も未着手**（git の同梱自体が未実装。`docs/notes/git-bundling.md` は調査のみ）
- 当時未実装だった設計項目: 操作ジャーナル（6.3）、スナップショットの間引き、4.3 手順 8（相手が追跡を外したファイルの書き戻し）、保存前のサイズ検査は、**いずれも Phase 1 で実装済み**
- core-github の単体テストが約 10 秒かかる（実時間で待つテストがある）: **解消済み**。macOS の CI で約 2 秒（8.2 の 3）

## 6. 判断した点

- パッケージマネージャは pnpm
- 最新の安定版を採用（SvelteKit 3、Vite 8、TypeScript 6.0.3、reqwest 0.13、httpmock 0.8）
- `update-ref` の制限は、設計書の当初の記述（`-d` のみ制限）より厳しくした

## 7. 安全上の不変条件の充足

| 不変条件 | 状況 |
| --- | --- |
| 1・2（禁止操作、`GitRunner` 経由） | 実行時に拒否。拒否テスト、敵対的入力のテスト、実リポジトリ上の実行テストで確認 |
| 3（状態変更前の復元点） | `save` / `pull` / `resolve` / `abort_merge` の開始前に作成（Phase 1 で、元に戻す・1 ファイルを戻す・取り消し・ファイル追加にも追加。design.md 6.2） |
| 4（一時インデックス） | core-safety の統合テストで作業フォルダとインデックスの不変を確認 |
| 5（`update-ref` の範囲） | `refs/hikae/` 配下に限定 |
| 6（未追跡ファイルを消さない） | 追跡解除は `rm --cached` のみ |
| 7（直列キュー） | Phase 0 の時点では未実装。**Phase 1 で実装済み**（`run_exclusive`。design.md 8.3、本書 8.2 の 1） |
| 8（グローバル設定に触れない） | `GIT_CONFIG_GLOBAL` を null デバイスに向け、テストで確認 |
| 9（トークンを出さない） | `AccessToken` と `DeviceCode` の Debug / Display、エラー文言で秘匿 |

## 8. Phase 0 以降の確認

2026-10-08 時点（コミット 59c1c94）の確認結果です。GitHub Actions の事実は `gh run list` / `gh run view` で確認できた範囲だけを書いています。時刻は UTC です。

### 8.1 CI の実行結果

- ワークフローは Windows（`windows-latest`）と macOS（`macos-latest`）の 2 OS で、`fail-fast: false`。手順は、`pnpm check`・`pnpm lint`・`pnpm test`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`pnpm tauri build --debug`。WebView 上の E2E テストは含まない
- 取得できた実行履歴は 15 件（2026-10-07 22:42 〜 2026-10-08 06:04）。最新の 2 件（4913453、59c1c94）を含む 11 件が、**両 OS とも全手順成功**
- 失敗は 4 件で、すべて**macOS の「Rust (fmt, clippy, test)」だけ**が失敗し、Windows は成功した。原因はいずれも `core-store` の同じテスト（`test_project_locks_different_ids_parallel`）。8.2 の 3 を参照
- 失敗の間にも成功した回がある（テストを直す前の 22:42、23:48、01:27 の 3 件）。同じコードで結果が揺れたことになり、実時間に依存したテストの不安定さを示している

### 8.2 Phase 1 の実装中に見つかって直した不具合と、再発防止

| # | 不具合 | 原因と修正 | 再発防止 |
| --- | --- | --- | --- |
| 1 | 操作キューが実際には直列化されていなかった | ロックを `spawn_blocking` の外側で取ると、クロージャがすぐ戻ってロックが本体の実行前に解放され、同一プロジェクトの操作が並行して走る。ロックを `spawn_blocking` の内側で取るようにした（`run_exclusive`）。修正後の形でコミットしたため、修正前のコードは履歴に残っていない（根拠はコミット e6185f3 のメッセージと `run_exclusive` のコメント） | 状態変更の操作を共通の `run_op` に集約し、必ず `run_exclusive` の内側で実行する。`ProjectLocks` に「同一 ID の最大同時実行数が 1」を確かめるテストがある |
| 2 | `history()` が許可リストにない `diff-tree` を呼び、履歴の一覧が実行時に失敗した | `GitRunner` は許可リストにないサブコマンドを実行前に拒否するため、一覧が毎回失敗した。`log -z` と `diff --numstat -z` で書き直した（コミット 2e9dc9b） | 許可リストにあるサブコマンドとフラグだけで組む。修正前に失敗する統合テスト（実リポジトリを一時ディレクトリに作る）を追加した。モックでは見つからない種類の不具合のため、新しい git の呼び出しには統合テストで実行経路を通す |
| 3 | 時間計測に依存したテストが macOS で不安定だった | `test_project_locks_different_ids_parallel` が、実行時間の差で並行実行を判定していた。macOS の CI で 4 回失敗した（8.1。成功した回もあり、結果が揺れた）。経過時間を測らず、双方が同時に臨界区間に入ったことを観測する形に変更した（コミット 1f67776） | 実時間の長さで合否を決めるテストを書かない。待つ場合は上限つきで「起きるべきことが起きたか」を観測する。`core-github` の実時間待ちのテストも、macOS の CI で約 10 秒から約 2 秒になった |
| 4 | `unsafe` の `MutexGuard` 偽装があった | 利用者の申告に基づく。現在の `src-tauri/app` と `src-tauri/crates` のソースに `unsafe` は無い（`grep` で確認）。取り除く前のコードは履歴に残っていない | `unsafe` に頼らず、ロックは `Mutex` の通常の使い方で保持する。委任した実装は、`unsafe` や実装の無い疑似実装が無いかを読んで確認する（CLAUDE.md の委任方針） |
| 5 | Device Flow のトークン取得のポーリングが毎回失敗した | トークンのエンドポイントは、`Accept: application/json` を付けないとフォーム形式で返し、解析に失敗していた（`InvalidTokenResponse`）。ヘッダーを付けるようにした（コミット 59c1c94） | ヘッダーが付いたときだけ JSON を返すモックのテストを追加した。実認証の手動確認は引き続き必要 |
| 6 | Phase 1 の実機確認で、変更一覧が出なかった | `app` 層が `git status` の出力を自前で解析していた。(1) `-z`（NUL 区切り）の出力を行単位で読んでいた、(2) 変更種別コードを取り違えていた、(3) 未追跡ファイルが欠落していた、(4) `-uall` が無く、未追跡のフォルダが 1 件にまとまっていた。解析を `core-ops` の `changes.rs` に移した（並行作業中。コミットは未了） | 統合テスト（`core-ops/tests/list_changes.rs`。実リポジトリを使う）を追加した。git 出力の解析は `app` 層に置かず、`core-*` crate でテストする |

共通の再発防止の方針は、(1) 時間計測に依存しないテストにする、(2) `GitRunner` の許可リストに入る引数だけを使う、(3) 実リポジトリを使う統合テストで実行経路を通し、実行時にしか出ない不具合を CI で検出する、の 3 つです。

### 8.3 現在も残っている未確認・未実装

- （解消）Device Flow の実認証は確認済み（2026-10-09）
- Windows・macOS の実機での通し確認、WebView 上の E2E テスト
- 署名・公証、自動更新（配布の直前に行う。git の同梱は Windows 実装済み、macOS は CI でのビルドまで確認済み）
- GitHub 上のリポジトリの作成（取得は実装済み）
- 実装済みの範囲の一覧は `docs/design.md` の 12章に記載
