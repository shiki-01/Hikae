# Git 同梱方式の検討

調査日: 2026-10-07

## 概要

Hikae は Tauri 2 デスクトップアプリで、Git 操作を同梱の git CLI 経由で実行する。本文書は Windows と macOS での git 同梱方法、配布サイズ、実装上の注意事項をまとめたものです。

## 1. Windows: MinGit

### 1.1 バージョンと配布形態

- **最新バージョン**: v2.56.0.windows.2（リリース日: 2026-10-05）
- **配布元**: https://github.com/git-for-windows/git/releases
- **提供形式**: exe インストーラ、tar.bz2、MinGit zip、PortableGit

### 1.2 MinGit 配布サイズ

| 形式 | ファイル名 | zip サイズ | 展開後サイズ | 用途 |
|---|---|---|---|---|
| 64-bit | MinGit-2.56.0.2-64-bit.zip | 39.8 MB | 91.7 MB | 推奨（標準） |
| arm64 | MinGit-2.56.0.2-arm64.zip | 37.9 MB | 未測定 | ARM 対応 |
| busybox 64-bit | MinGit-2.56.0.2-busybox-64-bit.zip | 35.3 MB | 未測定 | 軽量版 |
| busybox arm64 | MinGit-2.56.0.2-busybox-arm64.zip | 33.4 MB | 未測定 | ARM 軽量版 |

出典: https://github.com/git-for-windows/git/releases/tag/v2.56.0.windows.2

### 1.3 MinGit の構造と主なサイズ

展開後の MinGit 64-bit の内部構成（測定値: 91.7 MB）:

| ディレクトリ | サイズ | 備考 |
|---|---|---|
| ucrt64 | 64.6 MB | ライブラリとバイナリ（ほぼ Git CLI） |
| usr | 26.2 MB | MSYS2 ユーティリティ（sh, awk など） |
| etc | 0.66 MB | 設定ファイル |
| cmd | 0.17 MB | Windows 統合スクリプト |

### 1.4 最大ファイル分析

展開後の MinGit で占める領域が大きいファイル:

| ファイル | サイズ | 位置 | 削減可能性 |
|---|---|---|---|
| libSkiaSharp.dll | 8.98 MB | ucrt64/bin | 条件付き（Scalar 関連） |
| libcrypto-3-x64.dll | 5.15 MB | ucrt64/bin | 不可（HTTPS 必須） |
| av_libglesv2.dll | 4.24 MB | ucrt64/bin | 検討中 |
| git.exe | 4.21 MB | ucrt64/bin | 不可（必須） |
| msys-crypto-3.dll | 3.83 MB | usr/bin | 不可（HTTPS 必須） |
| msys-2.0.dll | 3.22 MB | usr/bin | 不可（MSYS2 実行環境） |
| git-remote-https.exe | 2.49 MB | ucrt64/bin | 不可（HTTPS 必須） |
| git-http-push.exe | 2.49 MB | ucrt64/bin | 不可（HTTP 必須） |
| scalar.exe | 2.46 MB | ucrt64/bin | 削減可（Scalar VFS は不要） |
| sh.exe | 2.34 MB | usr/bin | 不可（git hooks 実行に必須） |

出典: 手動測定（2026-10-07）

### 1.5 busybox 版との差分

MinGit の busybox 版は、MSYS2 ユーティリティ（awk, sed など）を削減した軽量版です。

- 64-bit busybox: 35.3 MB（標準版比 -4.5 MB）
- arm64 busybox: 33.4 MB（標準版比 -4.5 MB）

**検討**: Git CLI 単体で動作させる場合は busybox 版で十分な可能性があります。ただし `core.hooksPath` で実行される hook スクリプトが bash/sh に依存する場合は、フルサイズ版の `sh.exe` が必要です。Hikae では hook を実行しないため（`-c core.hooksPath=` で無効化）、busybox 版の検討価値があります。

### 1.6 ディレクトリ配置と環境変数

MinGit は単体では動作しません。以下のディレクトリ構造が必要です:

```
.
├── ucrt64/
│   ├── bin/
│   │   ├── git.exe
│   │   ├── git-remote-https.exe
│   │   └── ...
│   ├── libexec/
│   │   └── git-core/   (16 ファイルのハードリンク)
│   └── ...
├── usr/
│   ├── bin/
│   │   ├── sh.exe
│   │   ├── awk.exe
│   │   └── ...
├── etc/
└── cmd/
```

**環境変数**:
- `GIT_EXEC_PATH`: git-core サブコマンドの位置（通常 `ucrt64/libexec/git-core`）
- `GIT_TEMPLATE_DIR`: 初期化テンプレートの位置
- `GIT_CONFIG_NOSYSTEM`: 1 を設定してシステム設定を無視
- `GIT_CONFIG_GLOBAL`: ユーザー設定の位置を明示的に指定
- `HOME`: 一時ディレクトリ（ユーザーの ~/.gitconfig を参照させない）

### 1.7 Tauri への組み込み方

**推奨**: Tauri の `resources` 機構を使用:

```toml
# tauri.conf.json
{
  "build": {
    "resources": [
      "path/to/MinGit-2.56.0.2-64-bit"
    ]
  }
}
```

リソースは最終的なアプリパッケージに含まれ、`tauri::api::path::resource_dir()` で実行時に参照できます。

**注意**: `externalBin` は単一の実行ファイルを想定しており、MinGit のような複数ファイル依存構造には不向きです。

### 1.8 ライセンス

MinGit（Git for Windows）は GPLv2 です。

- **ライセンス文書**: 各リリースの zip に `LICENSE.txt` が含まれます
- **同梱要件**: ライセンス文書をアプリ内またはドキュメントに含める必要があります
- **出典表示**: "Git for Windows" の名前と配布元（git-for-windows/git）を明示します

## 2. macOS: git 同梱方式の検討

### 2.1 選択肢の比較

| 方式 | メリット | デメリット | 検討度 |
|---|---|---|---|
| git ソースビルド（universal binary） | 完全な制御、最小化可能、再配布可 | ビルド環境とメンテナンスコスト | **推奨** |
| git-osx-installer 成果物の再配布 | ビルド不要、公式配布物 | 提供形式の制限、ライセンス確認必要 | 検討中 |
| Homebrew 版のコピー | すぐ入手可 | 再配布不可、依存ライブラリ散在、ライセンス問題 | 非推奨 |
| Xcode CLT の git に依存 | 配布サイズなし | ユーザーが CLT インストール必須、オフラインで動作不可 | 不可 |

### 2.2 自前ビルドの推奨構成

**ビルドオプション** (最小化方針):

```bash
./configure \
  --prefix=/path/to/git-install \
  --enable-pthreads \
  --with-openssl \
  --with-curl \
  --without-tcltk \
  --without-perl \
  --without-python \
  NO_GETTEXT=1 \
  NO_LIBINTL=1 \
  RUNTIME_PREFIX=yes

make -j$(nproc)
make install
```

**各オプション説明**:
- `--without-tcltk`: Git GUI（gitk）サポート削減（GUI は不要）
- `--without-perl`: Perl スクリプトサポート削減（git-svn など不要）
- `--without-python`: Python サポート削減（不要）
- `NO_GETTEXT=1`: 多言語サポート削減
- `NO_LIBINTL=1`: Intl ライブラリ削減
- `RUNTIME_PREFIX=yes`: バイナリが相対パスでライブラリを参照（配布向け重要）

### 2.3 Universal Binary 化

arm64 (Apple Silicon) と x86_64 両対応を実現するには:

```bash
# x86_64 ビルド
./configure --prefix=../git-x86_64 CFLAGS="-arch x86_64" ...
make; make install

# arm64 ビルド
./configure --prefix=../git-arm64 CFLAGS="-arch arm64" ...
make; make install

# Universal バイナリ結合
lipo -create ../git-x86_64/bin/git ../git-arm64/bin/git \
     -output ../git-universal/bin/git
```

**注意**: ライブラリ（libcurl, libz, libcrypto など）も同様に universal バイナリ化する必要があります。

### 2.4 配布サイズの見込み

未測定ですが、以下の要素により変動します:

- **削減なし**: 約 150～200 MB（ライブラリ含む）
- **積極削減**: 約 50～80 MB（NO_GETTEXT, ライブラリ最小化）

**不確定要素**: OpenSSL のバージョンと同梱方法（動的リンク vs 静的リンク）により大きく変動します。

### 2.5 署名と公証

macOS では配布前に以下の処理が必須です（Big Sur 11 以降）:

- **Code Signing**: Developer ID Certificate で署名
- **Notarization**: Apple へ提出後、公証を取得

```bash
# 署名（各バイナリ）
codesign --force --verify --verbose \
  --sign "Developer ID Application: <Team Name>" \
  --timestamp \
  --options runtime \
  path/to/git

# 公証（Tauri のビルド成果物全体）
# Tauri 2 は xcrun notarytool で自動処理可能
```

**hardened runtime** オプション (`--options runtime`) が必須です。これにより:
- code signing 検証時の遅延
- スクリプト実行権限の明示

### 2.6 git-core 下の複数ファイル

libgit2 互換の実装や実際の git ビルドでは、`libexec/git-core/` 下に多数のサブコマンドバイナリ（あるいはハードリンク）が配置されます:

```
libexec/git-core/
├── git-add
├── git-branch
├── git-commit
└── ... (100+ ファイル)
```

Windows の MinGit では 16 ファイルのハードリンクですが、macOS ソースビルドではバイナリサイズに応じて実ファイルまたはシンボリックリンクになります。**シンボリックリンクの使用により配布サイズを大幅削減可能**です。

### 2.7 ライセンス

git ソースからビルドした場合も GPLv2 です。

- **ビルド時の通知**: ソースの LICENSE ファイルを保持
- **同梱要件**: Windows と同様にライセンス文書を含める

## 3. 共通事項

### 3.1 バージョン更新方針

未決事項ですが、検討すべき選択肢:

1. **定期更新**: セキュリティパッチ適用時に MinGit / git ビルドを更新（年 2～4 回程度）
2. **LTS 方式**: 古いバージョンで動作検証済みに固定（サポート終了時のみ更新）
3. **ユーザー選択**: ユーザーが環境の git を使用できる fallback メカニズム

**Hikae での推奨**: 定期更新。セキュリティフィックスをタイムリーに反映する必要があります。

### 3.2 HTTPS サポート

git の HTTPS 通信には以下が必須です:

- **通信ライブラリ**: libcurl
- **TLS ライブラリ**:
  - Windows MinGit: OpenSSL（libcrypto-3-x64.dll）
  - macOS: OpenSSL（自前ビルド時）or SecureTransport（Apple 標準）
- **CA 証明書**: 
  - Windows: Windows の CA ストアを参照（libcurl で自動）
  - macOS: macOS の Keychain を参照（自動）

**注意**: CA 証明書が古い場合、HTTPS 接続に失敗します。ユーザー環境の更新を前提とします。

### 3.3 ユーザー環境の git との衝突回避

Hikae がユーザーのシステム git と衝突しないようにするための対策:

1. **絶対パス呼び出し**: `GitRunner` から同梱 git を明示的なフルパス呼び出し（`/path/to/bundled/git.exe`）で実行
2. **環境変数の制御**: git 実行時に以下を設定:
   - `GIT_CONFIG_NOSYSTEM=1`: システム git 設定（`/etc/gitconfig`）を無視
   - `GIT_CONFIG_GLOBAL=/path/to/app/git/config`: グローバル設定をアプリ内に閉じ込め
   - `HOME=/path/to/temp`: ユーザーの `~/.gitconfig` を参照させない
3. **PATH 分離**: git 実行プロセスに PATH を明示的に指定せず、git 本体への絶対パス呼び出しのみ

### 3.4 インストーラ（NSIS / dmg）への影響

**Windows NSIS**:
- MinGit zip をアプリケーションディレクトリに展開するステップを追加
- インストーラサイズ: +35～40 MB（圧縮率により変動）

**macOS dmg**:
- git バイナリをアプリバンドル内（`Contents/Resources/` など）に配置
- 署名・公証処理が必須（Tauri が自動化）

### 3.5 アプリ全体への配布サイズ上乗せ

**Windows**:
- MinGit 標準版: +35～40 MB
- MinGit busybox 版: +30～35 MB（ただし hook 実行時に要確認）

**macOS**:
- git 自前ビルド（削減なし）: +150～200 MB
- git 自前ビルド（積極削減）: +50～80 MB
- 公証によるオーバーヘッド: +5～10 MB（署名付きコード）

## 4. 未確認・決定待ちの事項

以下の項目については、実装フェーズで決定が必要です:

1. **macOS git の自前ビルル vs git-osx-installer 利用の最終判断**
   - git-osx-installer が再配布可能であるか、ライセンス上確認が必要
   - ビルド環境（CI/CD）とメンテナンスコストの評価

2. **MinGit busybox 版の検証**
   - Hikae で hook を実行しないため、busybox 版で動作するか実装時に確認
   - 確認できれば配布サイズ 4～5 MB 削減可能

3. **OpenSSL vs 標準 TLS ライブラリの選択（macOS）**
   - SecureTransport 使用時のビルドオプションと配布サイズ
   - TLS 1.3 対応状況の確認

4. **git バージョン更新タイミングの方針**
   - セキュリティ脆弱性の監視体制
   - LTS 方式と定期更新の cost/benefit 分析

5. **CA 証明書の更新戦略**
   - アプリリリースサイクルと git CA 証明書の同期
   - オフライン環境での HTTPS 接続対応の有無

6. **ユーザー環境の git を使用する fallback メカニズム**
   - 同梱 git が起動失敗時に PATH の git を試す仕様の要否
   - safety 層での検証（コマンドホワイトリスト適用）

## 5. 推奨方式（Phase 1 での採用案）

### Windows

**推奨**: MinGit 64-bit zip をリソースに展開

- バージョン: v2.56.0.2（またはそれ以降の最新）
- 形式: 64-bit zip（busybox 版は hook 未実行確認後に検討）
- 配置: Tauri `resources/` ディレクトリ
- サイズ: app 配布に +35～40 MB
- ライセンス: LICENSE.txt を app に同梱

### macOS

**推奨（一次案）**: git ソースからの universal binary 自前ビルド

- 理由: 完全な制御、削減可能性、GPLv2 準拠明確
- 構成: `--without-tcltk`, `--without-perl` で最小化
- Universal: lipo による arm64/x86_64 統合
- 署名・公証: Tauri で自動化
- サイズ: 削減オプション適用時 50～80 MB 見込み

**検討事項**: git-osx-installer の再配布ライセンスを確認後、ビルド不要なら切り替え検討

### 共通

- **バージョン更新**: 定期更新（年 2～4 回、セキュリティパッチ時）
- **環境分離**: GIT_CONFIG_NOSYSTEM, HOME, 絶対パス呼び出し
- **ライセンス同梱**: GPLv2 通知をアプリに含める
- **HTTPS CA**: システム証明書に依存（ユーザー環境の更新を前提）

## 6. 参考資料

- MinGit リリース: https://github.com/git-for-windows/git/releases
- git ソース: https://github.com/git/git
- git-osx-installer: https://sourceforge.net/projects/git-osx-installer/files/
- Tauri 2 リソース機構: https://tauri.app/v2/reference/api/
- Git for Windows ドキュメント: https://git-scm.com/download/win

## 7. 実装結果（2026-10-08）

### 7.1 実装の構成

| 項目 | 内容 |
|---|---|
| 探索 | `core-git` の `GitRunner::bundled(resource_dir)`。優先順位は (1) `HIKAE_GIT_PATH`（存在するファイルのときだけ採用。空や存在しない値は無視）、(2) `<resource_dir>/git/` の同梱 git、(3) PATH 上の git。`from_path_env()`（テスト用）も `HIKAE_GIT_PATH` を見る |
| `HIKAE_GIT_PATH` が同梱 git を指すとき | `<root>/cmd/git.exe`（Windows）または `<root>/bin/git`（Unix）の形で、`<root>` に exec-path があれば同梱構成と推定し、同じ環境変数を足す |
| app | `setup` で `resource_dir` を `OnceLock` に保持し、`git_runner()` が `GitRunner::bundled` を使う。credential helper の設定は従来どおり |
| Windows の取得 | `scripts/fetch-git-windows.mjs`。公式リリース `v2.56.0.windows.2` の `MinGit-2.56.0.2-64-bit.zip`、SHA-256 `da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be`（`gh api` の digest と一致を確認）をスクリプトに固定。検証に失敗したら破棄して終了コード 1（不一致のハッシュで実際に失敗することを確認）。展開は PowerShell の `Expand-Archive`。展開後に不要ファイルを削り、同梱 git で最小の流れを実行してから `src-tauri/app/resources/git/` に移す。版が一致していれば再取得しない（`.hikae-git-version`）。`tauri.conf.json` の `beforeDevCommand` / `beforeBuildCommand` から呼ぶ |
| Windows の同梱 | `bundle.resources` で `resources/git` → `git`、`resources/NOTICE.txt` → `NOTICE.txt`。`git/` の中身は `.gitignore` 済み（`.gitkeep` だけ追跡。`tauri-build` が resources の存在を要求するため） |
| macOS | `scripts/build-git-macos.sh`。kernel.org の `git-2.56.0.tar.xz`（SHA-256 `26c56c29…89d3`。kernel.org の `sha256sums.asc` と一致を確認）を固定。arm64 / x86_64 を別々にビルドして `lipo`。CI は `workflow_dispatch` の `bundle-git-macos` ジョブがビルドしてキャッシュに保存し、`check` ジョブの macOS 脚が復元する |

### 7.2 Windows の同梱 git の実測

- 採用: **通常版** `MinGit-2.56.0.2-64-bit.zip`（39.8 MB）。busybox 版は採用しない（理由は 7.4）
- 展開後の構成: `cmd/git.exe`（ラッパー）、`ucrt64/bin/`（git.exe、git-remote-https.exe、DLL）、`ucrt64/libexec/git-core/`（スクリプト類）、`ucrt64/share/git-core/templates`、`usr/bin/`（sh.exe ほかの MSYS2 コマンド）、`etc/`、`LICENSE.txt`。`git-remote-https.exe` などは `ucrt64/bin` にあり、`etc/libexec-moved.txt` に移動した旨が書かれている
- 削ったファイル（`ucrt64/bin` の Avalonia / System / Microsoft の各 DLL、GitHub・GitLab・Bitbucket の DLL、SkiaSharp・HarfBuzzSharp・av_libglesv2・msalruntime・gcmcore、git-credential-manager 一式、git-askpass、git-askyesno、blocked-file-util、docx-strip-pii、docx2txt、scalar、git-update-git-for-windows、`libexec/git-core/git-credential-wincred.exe`、`share/doc`、`share/bash-completion`）: 展開後 93 MB から **61 MB** へ（`ucrt64` 33 MB、`usr` 27 MB）。`usr/bin` はこれ以上削っていない（sh.exe が使う msys-*.dll の依存を確かめきれていないため）
- 生成物（`tauri build --debug`）: NSIS インストーラ 25.7 MB、MSI 44.5 MB（デバッグビルド）
- 調査ノート 1.6 の構成図の「`ucrt64/libexec/git-core` の 16 ファイルのハードリンク」は実際と異なる。実行ファイルは `ucrt64/bin` にあり、`libexec/git-core` にはスクリプト類と wincred だけがある

### 7.3 子プロセスに渡す環境変数

`GitRunner` は従来どおり親の環境を引き継がず（`env_clear`）、次を設定する。ユーザーのグローバル設定（`~/.gitconfig`）や親プロセスの環境は変更しない。

| 変数 | 値 | 備考 |
|---|---|---|
| `PATH` | 同梱の `ucrt64/bin`、`usr/bin`（Unix は `bin`）を先頭に足した値 | 同梱 git のときだけ。`!` 形式の credential helper を実行する sh と、git-remote-https の依存 DLL を見つけるため |
| `GIT_EXEC_PATH` | `<git>/ucrt64/libexec/git-core`（Unix は `<git>/libexec/git-core`） | 同梱 git のときだけ。存在するときだけ設定 |
| `GIT_TEMPLATE_DIR` | `<git>/ucrt64/share/git-core/templates` | 同上 |
| `GIT_CONFIG_NOSYSTEM` | `1` | 既存。MinGit の `etc/gitconfig`（`credential.helper=manager`、`core.autocrlf=true`）を読まないために必須 |
| `GIT_CONFIG_GLOBAL` | `NUL`（Unix は `/dev/null`） | 既存。変更なし |
| `GIT_TERMINAL_PROMPT` / `LC_ALL` / `GIT_OPTIONAL_LOCKS` | `0` / `C` / 読み取り系のみ `0` | 既存。変更なし |

`HOME` は設定していない（`GIT_CONFIG_GLOBAL` で足りる）。MinGit は `RUNTIME_PREFIX` でビルドされており、`cmd/git.exe` 経由なら `GIT_EXEC_PATH` が無くても自分の位置から exec-path を解決することを確認した（空の環境で `--exec-path` が同梱の `ucrt64/libexec/git-core` を指す）。`GIT_EXEC_PATH` の設定は、念のための明示。**`ucrt64/bin/git.exe` を直接呼ぶ構成は採らない**（環境を空にするとクラッシュした）。

### 7.4 確認したこと

- 親の環境を空にした状態（`env -i`、PATH なし、`GIT_CONFIG_GLOBAL=NUL`、`GIT_CONFIG_NOSYSTEM=1`）で、同梱 git により init、config、add、commit、log が通る
- 同じ状態で、status（`--porcelain=v2 -z --branch`）、log、ls-tree、cat-file、show、rev-list、for-each-ref、merge-base、check-ignore、ls-files、write-tree、commit-tree、update-ref、read-tree、diff、restore、rm --cached、remote add、push、fetch、clone（ローカルの bare リポジトリ）が成功する
- `HIKAE_GIT_PATH` に同梱 git を指して `cargo test -p core-git -p core-safety -p core-ops` を実行し、全テストが通る（マージの競合、一時インデックス、credential helper を含む）。ただしテストの準備コードの一部が `git` を直接呼ぶため、PATH から git を外した状態では 10〜13 件が準備の段階で失敗する（`GitRunner` の問題ではない）。PATH を最小にしても、`credential_helper` のテスト（`!` 形式の helper を sh で実行）は同梱 git で通った
- `git-remote-https` の存在と、実際の呼び出し（接続不能なローカルアドレスへの `ls-remote` が curl のエラーで終わる）を確認。実 GitHub への通信はしていない
- busybox 版: init、add、commit、log は通る。しかし `!` 形式の alias / credential helper が `-c: applet not found` で失敗する（sh が無く、`busybox.exe -c ...` で呼ばれるため）。開発 PC では PATH 上の別の git に sh があり、普通に実行すると成功してしまうので、**環境を空にして**確認した。`busybox.exe` を `sh.exe` に複製すると動くが、公式の構成から外れるため、通常版を採用した（zip で 4.5 MB の差）

### 7.5 macOS の判断

- TLS と HTTP は **macOS 標準の libcurl に動的リンク**する（`/usr/lib/libcurl.4.dylib`、Apple の証明書ストア）。OpenSSL を同梱・静的リンクしない（`NO_OPENSSL`）。理由: 同梱物と更新対象が減る、universal 化で依存ライブラリを lipo する必要がない、CA 証明書が OS の更新に追従する。注意点: macOS 標準 libcurl の挙動は OS の版に依存する（最小は 11.0）
- `RUNTIME_PREFIX=yes`、`NO_GETTEXT`、`NO_TCLTK`、`NO_PERL`、`NO_PYTHON`、`NO_EXPAT`（`git-http-push` を作らない）、`INSTALL_SYMLINKS`（`libexec/git-core` の実体を 1 つにする）、`SKIP_DASHED_BUILT_INS`。configure は使わず、make の変数で渡す
- 調査ノート 2.2 の `--with-openssl` は採らなかった（上記の理由）。`--without-perl` などは make の `NO_PERL` などに対応する

### 7.6 未検証事項

- macOS: `scripts/build-git-macos.sh` は `bash -n` の構文確認のみ。実際のビルド、universal 化、CI の `bundle-git-macos` ジョブとキャッシュの受け渡しは**未確認**。ビルド後のサイズも未測定。Tauri の resources コピーが `libexec/git-core` のシンボリックリンクをどう扱うか（実体に展開されてサイズが増えないか）は**未確認**
- Windows: git が入っていない実機での起動確認は未実施。インストール後のアプリや `tauri dev` で `resource_dir` が想定のフォルダを指し、同梱 git が使われることの画面上の確認も未実施（`tauri build --debug` の成功と、resources が `target/debug/git` とインストーラに含まれることまで）
- `resource_dir` が `\\?\` 付きのパスで返る場合に備え、接頭辞を外す処理を入れたが、実際にその形で返るかは未確認
- 実際の GitHub に対する HTTPS の clone / fetch / push（同梱の libcurl と OpenSSL、Windows の証明書ストア）は未確認
- Windows の arm64 版（MinGit の arm64）は対象外
- 署名・公証、同梱 git の版更新の監視は未着手
