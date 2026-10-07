# Phase 0 報告

作成日: 2026-10-08

Phase 0（技術検証）の結果です。環境は Windows 11 のみで、macOS と GitHub Actions 上の CI は未検証です。

## 1. 構成

- フロントエンド: SvelteKit 3、Svelte 5、adapter-static の SPA（`ssr = false`）、TypeScript 6.0.3、Vite 8
- Master CSS: `@master/css` と `@master/css.vite` を `2.0.0-rc.88` に完全固定。トークンは `src/lib/styles/tokens.css`（light/dark、値は仮）
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

- CI（`.github/workflows/ci.yml`）は一度も実行していない。macOS のビルドとテストも未検証
- ウィンドウ内から Rust コマンドを呼ぶ処理（IPC）は未実装で、コマンド登録の実動作は未検証
- Device Flow の実認証（手動確認が必要）
- macOS の git 同梱サイズ、署名・公証
- 未実装の設計項目: 操作ジャーナル（6.3）、スナップショットの間引き、4.3 手順 8（相手が追跡を外したファイルの書き戻し）、保存前のサイズ検査
- core-github の単体テストが約 10 秒かかる（実時間で待つテストがある）

## 6. 判断した点

- パッケージマネージャは pnpm
- 最新の安定版を採用（SvelteKit 3、Vite 8、TypeScript 6.0.3、reqwest 0.13、httpmock 0.8）
- `update-ref` の制限は、設計書の当初の記述（`-d` のみ制限）より厳しくした

## 7. 安全上の不変条件の充足

| 不変条件 | 状況 |
| --- | --- |
| 1・2（禁止操作、`GitRunner` 経由） | 実行時に拒否。拒否テスト、敵対的入力のテスト、実リポジトリ上の実行テストで確認 |
| 3（状態変更前の復元点） | `save` / `pull` / `resolve` / `abort_merge` の開始前に作成 |
| 4（一時インデックス） | core-safety の統合テストで作業フォルダとインデックスの不変を確認 |
| 5（`update-ref` の範囲） | `refs/hikae/` 配下に限定 |
| 6（未追跡ファイルを消さない） | 追跡解除は `rm --cached` のみ |
| 7（直列キュー） | `app` 層に未実装（Phase 1） |
| 8（グローバル設定に触れない） | `GIT_CONFIG_GLOBAL` を null デバイスに向け、テストで確認 |
| 9（トークンを出さない） | `AccessToken` と `DeviceCode` の Debug / Display、エラー文言で秘匿 |
