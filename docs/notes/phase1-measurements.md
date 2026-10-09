# Phase 1 の性能・サイズの計測

計測日: 2026-10-09。対象コミット: f2a50c7（作業ツリーの未コミットの変更を含む。計測用の `src-tauri/crates/core-ops/tests/perf.rs` のみが追加分）。設計書 1.4（非機能要件）の目標に対する実測です。

## 1. 結論

| 項目 | 目標（1.4） | 結果 | 判定 |
| --- | --- | --- | --- |
| 状態表示の更新（1 万ファイル） | 1 秒以内 | 変更なし 75ms、100 件変更 136ms（`list_changes`）。`project_status` 相当（4 回の git 呼び出し）で 329〜474ms | 達成 |
| 状態表示の更新（合計 5GB） | 1 秒以内 | **5GB では未計測**。500MB（100 ファイル × 5MB）で、変更なし 48ms、全 100 ファイルの内容を書き換えた直後 約 1.0 秒 | 変更なしは達成。大量の書き換え直後は、書き換えた量に比例して 1 秒前後かかる（5GB は未確認） |
| インストーラの大きさ（git・LLM を除く） | 30MB 以下 | リリースビルドの NSIS 22.9MB、MSI 37.0MB（いずれも git 込み）。git を除くと NSIS 約 6MB、MSI 約 9MB（見積り） | 達成（見積り） |
| コールドスタート | 2 秒以内 | **未計測** | 判定不能（4 章） |

状態表示の目標は、1 万ファイルでは余裕を持って達成しています。`core.untrackedCache` と `core.fsmonitor` は、今回の構成では効果が小さく、採用しない提案です（5 章）。

一方、目標には無い別の箇所に、利用者が待たされうる遅さが見つかりました。実装の変更は行わず、提案として 5 章に書きました。

- 1 万ファイルの初回保存が 430 秒（約 7 分 10 秒）かかる。1 ファイルあたり 43ms で、1,000 ファイルのとき（18.5ms）より増え方が速い
- 履歴の 1 ページ（100 件）の取得が約 4 秒かかる（保存 1 件ごとに git を 1 回起動するため）
- 500MB を書き換えた後の保存は約 31 秒、自動保存は約 22 秒かかる

## 2. 環境と手順

### 2.1 環境

| 項目 | 内容 |
| --- | --- |
| CPU | 13th Gen Intel Core i7-13700（16 コア 24 スレッド） |
| メモリ | 15.6GB |
| ディスク | SSD（Hanye HE70、2TB）、`C:` の空きは約 360GB |
| OS | Windows 11 Home 10.0.26200 |
| git | 同梱の MinGit 2.56.0.windows.2（`src-tauri/app/resources/git`。`GitRunner::bundled` が使う）。PATH 上の git（2.49.0.windows.1）は使っていない。ログの `入手元=Bundled` で確認 |
| ビルド | `cargo test`（debug プロファイル）。Rust 側の処理は最適化なしだが、時間の大半は git の子プロセスで、git は最適化済みの実行ファイル |
| その他 | ウイルス対策ソフトの有無・除外設定は調べていない（Windows の標準の保護が有効な可能性がある） |

### 2.2 手順

計測は `src-tauri/crates/core-ops/tests/perf.rs`（`#[ignore]` 付き。通常の `cargo test` では動かない）で行いました。時間のしきい値は assert せず、変更の件数など結果の正しさだけを assert します。一時ディレクトリに実リポジトリを作り、終了時に削除します（実行の前後で `%TEMP%` の `.tmp*` ディレクトリの数が同じ 441 であることを確認）。

```text
cd src-tauri
HIKAE_PERF_FILES=10000 HIKAE_PERF_HISTORY=0 cargo test -p core-ops --test perf -j 2 -- --ignored --nocapture --test-threads=1 perf_many_files
HIKAE_PERF_FILES=200 HIKAE_PERF_HISTORY=130 cargo test -p core-ops --test perf -j 2 -- --ignored --nocapture --test-threads=1 perf_many_files
HIKAE_PERF_LARGE_FILES=100 HIKAE_PERF_LARGE_MB=5 cargo test -p core-ops --test perf -j 2 -- --ignored --nocapture --test-threads=1 perf_large_total
```

3 回に分けた理由は、10 分の上限に収めるためです。履歴 100 件を作るには 130 回の保存が要り（1 回 約 1.4 秒）、1 万ファイルの初回保存だけで 7 分かかるため、同じ実行には入りません。

| 実行 | 内容 | 所要時間 |
| --- | --- | --- |
| A | 1 万ファイル（深さ 3〜4、5 件に 1 件は日本語名、500 件に 1 件は 1MB の乱数、ほかは約 2KB のテキスト） | 503 秒 |
| B | 200 ファイル + 保存を 130 回重ねた履歴（履歴の取得の計測用） | 240 秒 |
| C | 500MB 相当（100 ファイル × 5MB の乱数。圧縮されにくい画像や動画を想定） | 222 秒 |

ログ上の場面名は、実行 B が「1万ファイル」、実行 C が「5GB相当」と出ていますが、実際のファイル数は上の表のとおりです（場面名はテストの固定の文字列）。

計測する項目:

- (a) 初回保存（`save`）
- (b) 変更なしの `list_changes`（5 回）
- (c) 等間隔の 100 ファイルを書き換えた後の `list_changes`（5 回。実行 C は 100 ファイル全部が対象）
- (d) 自動保存（`auto_snapshot`）の作成。続けて、同じ内容で呼んだとき（作らない場合）
- (e) `project_status` 相当（`sync_state` + `conflicts` + `list_changes` + `commit_count`。3 回）
- (f) 履歴のページ取得（100 件、`offset=0` と `offset=100`、各 3 回）
- 上記を、設定なし（基準）、`core.untrackedCache=true`、`core.fsmonitor=true` + `core.untrackedCache=true` の 3 つで比較（(a)・(d)・(e)・(f) は基準のみ）。設定は計測用リポジトリのローカル設定に限り、ユーザーのグローバル設定は読まない・書かない

表の値は中央値です（最小・最大は付録のログ）。`list_changes` はアプリと同じ呼び方（`GIT_OPTIONAL_LOCKS=0`）です。

## 3. 結果

### 3.1 1 万ファイル（実行 A）

| 項目 | 基準 | untrackedCache | fsmonitor + untrackedCache |
| --- | --- | --- | --- |
| (b) 変更なし `list_changes` | 75ms | 75ms | 53ms |
| (c) 100 件変更 `list_changes` | 136ms（1 回目 199ms） | 135ms（1 回目 258ms） | 105ms（1 回目 198ms） |
| 参考: 変更なし `status`（ロックを許す形） | 68ms | 67ms | 43ms |
| 参考: 100 件変更 `status`（同上） | 114ms | 111ms | 91ms |
| 参考: 100 件変更の保存 | 4.8 秒 | 4.4 秒 | 4.5 秒 |

| 項目 | 結果 |
| --- | --- |
| (a) 初回保存 | **430.2 秒**（参考: ファイル生成 36.4 秒。保存データは 23MiB） |
| (d) 自動保存の作成（100 件変更後） | 3.0 秒 |
| (d) 自動保存（同じ内容で再び呼ぶ。作らない） | 1.6 秒 |
| (e) `project_status` 相当、100 件変更 | 474ms（最小 445ms、最大 691ms） |
| (e) `project_status` 相当、変更なし | 329ms（最小 308ms、最大 342ms） |
| (f) 履歴（保存 5 件しかない状態） | `offset=0` 565ms（1 回目 約 1.5 秒の外れ値あり） |

### 3.2 履歴 100 件（実行 B。200 ファイル、保存 135 件）

| 項目 | 結果 |
| --- | --- |
| 履歴用の保存 130 回（各回 1 ファイルを書き換えて保存） | 185.6 秒（1 回あたり 約 1.4 秒） |
| (f) `offset=0 limit=100`（100 件を返す） | 中央値 4,010ms（最小 3,970ms、最大 6,238ms） |
| (f) `offset=100 limit=100`（35 件を返す） | 中央値 1,684ms（最小 1,578ms、最大 2,473ms） |

1 件あたり約 40ms で、件数に比例します。実装は、返す 1 件ごとに `git diff --numstat` を 1 回起動して変更ファイル数を数えています（`core-ops/src/history.rs`）。Windows ではプロセスの起動が 1 回 数十 ms かかるため、これが支配的と考えられます（プロファイルはしていない）。

### 3.3 500MB 相当（実行 C。100 ファイル × 5MB）

| 項目 | 基準 | untrackedCache | fsmonitor + untrackedCache |
| --- | --- | --- | --- |
| (b) 変更なし `list_changes` | 48ms | 44ms | 44ms |
| (c) 全 100 ファイルの内容を書き換え `list_changes` | 1,015ms | 1,039ms | 1,019ms |
| 参考: 全 100 ファイルを書き換えた後の保存 | 30.8 秒 | 27.3 秒 | 35.5 秒 |

| 項目 | 結果 |
| --- | --- |
| (a) 初回保存 | 23.3 秒（参考: ファイル生成 1.7 秒。保存データは 500MiB） |
| (d) 自動保存の作成（全 100 ファイル書き換え後） | 21.7 秒 |
| (d) 自動保存（同じ内容で再び呼ぶ。作らない） | 4.7 秒 |
| (e) `project_status` 相当、全 100 ファイル書き換え | 3,156ms |
| (e) `project_status` 相当、変更なし | 206ms |
| (f) 履歴（保存 5 件。各回に 500MB の差分あり） | `offset=0` 3,725ms |

この実行の (c) は、書き換えたファイルがすべて 5MB で、大きさが変わらず更新時刻だけが変わるため、git が内容を読んで比べています。約 1 秒は 500MB を読む時間で、書き換えたデータ量に比例します（約 2ms/MB）。変更がない通常の状態では、ファイルの大きさと更新時刻だけを見るため、ファイルの総量に依存しません（5GB でも変更なしの `list_changes` は同程度と推定。未計測）。

### 3.4 1 万ファイル規模の補助実行（1,000 ファイル）

実行 A の前に、計測の見積りのため 1,000 ファイルで試走しました（初回保存 18.5 秒、変更なし `list_changes` 65ms、100 件変更 74ms など）。結果は付録のログにあります。初回保存は 1,000 ファイルで 18.5ms/ファイル、1 万ファイルで 43ms/ファイルで、ファイル数に対して直線より速く増えています。

## 4. インストーラの大きさと起動時間

### 4.1 インストーラ

リリースビルドは今回実行していません。リポジトリには、2026-10-09 13:33 に作られたリリースビルドの成果物と、2026-10-08 22:50 に作られた debug ビルドの成果物が既にあり、それを測りました（リリースビルドの作成時刻は、この計測の開始前です。コミット f2a50c7 のコードから作られたかどうかは、成果物からは確認していません）。

| 成果物 | 大きさ |
| --- | --- |
| リリース NSIS（`src-tauri/target/release/bundle/nsis/Hikae_0.0.1_x64-setup.exe`） | 22,877,893 バイト（22.9MB） |
| リリース MSI（`.../bundle/msi/Hikae_0.0.1_x64_en-US.msi`） | 37,024,268 バイト（37.0MB） |
| リリースの `hikae.exe` | 14,786,560 バイト（14.8MB） |
| debug NSIS | 26,925,909 バイト（26.9MB） |
| debug MSI | 46,694,940 バイト（46.7MB） |
| debug の `hikae.exe` | 34,655,744 バイト（34.7MB） |

同梱の git（`src-tauri/app/resources/git`。MinGit 2.56.0.windows.2）:

| 項目 | 大きさ |
| --- | --- |
| 展開後 | 62,507,568 バイト（62.5MB、304 ファイル）。内訳は `ucrt64` 32.6MB、`usr` 26.2MB、`etc` 0.7MB、`cmd` 0.2MB |
| zip 圧縮（deflate） | 28,018,659 バイト（28.0MB） |
| xz 圧縮（LZMA2。NSIS の圧縮方式に近い） | 16,648,476 バイト（16.6MB） |

`hikae.exe` を同じ xz で圧縮すると 4,105,528 バイト（4.1MB）です。

**git を除いたインストーラの見積り**:

- NSIS: 22.9MB − git の LZMA 圧縮分（約 16.6MB）= **約 6MB**。逆算の確認として、`hikae.exe` 4.1MB + git 16.6MB = 20.8MB で、実測 22.9MB との差 2.1MB はインストーラ本体・アイコン・圧縮方式の違いの範囲
- MSI: 37.0MB − git の deflate 圧縮分（約 28.0MB）= **約 9MB**（MSI のキャビネットは deflate 系のため zip で見積もった）

どちらも、設計書 1.4 の「LLM・git バイナリを除くインストーラ 30MB 以下」を大きく下回ります。git を含めた実際の配布物でも、NSIS は 22.9MB で 30MB 以下です（MSI は 37.0MB で超える。目標は git を除く値なので、超過ではない）。macOS の配布物（dmg）と同梱 git の大きさは未計測です。

注意: この見積りは Windows の x64 のみ。LLM（段階 1〜2）を同梱しない前提。署名・自動更新の組み込みで大きさが変わる可能性は小さい。

### 4.2 起動時間

**未計測**です。コールドスタートの計測には、リリースビルドのアプリを、OS のファイルキャッシュが無い状態で起動し、WebView2 の初期化を含めてウィンドウが操作可能になるまでの時間を測る必要があります。この環境では、GUI の操作と画面の描画完了の判定を自動で行う手段が無く（WebView 上の E2E テストが無い。12 章）、キャッシュを落とす手順（再起動）も取れないため、実施していません。配布の直前の実機試験で測ることを提案します。

## 5. 提案（実装の変更はしていない）

### 5.1 `core.untrackedCache` と `core.fsmonitor` を採用するか

**採用しない**ことを提案します。

- `core.untrackedCache` は、今回の 3 つの構成（1 万ファイル、500MB）のどれでも効果がありませんでした（基準 75ms に対し 75ms、136ms に対し 135ms）。この機能は、未追跡のファイルが多いフォルダで未追跡の探索を省く仕組みです。今回の計測は大半が追跡済みのファイルで、効果が出る条件ではありません。未追跡のファイルが数千件ある場合は効果が出うるため、その条件での計測は未実施です
- `core.fsmonitor` は、1 万ファイルで約 20〜30% 短くしました（75ms → 53ms、136ms → 105ms）。しかし、基準でも目標（1 秒）の 1/7 以下で、絶対値の差は 20〜30ms です。一方で、リポジトリごとに常駐の daemon が起動します。計測用のテストでも、daemon を止めないと一時ディレクトリを削除できませんでした。フォルダの削除・移動・「一覧から外す」・クラウド同期フォルダとの組み合わせで、ロックの原因になるおそれがあります
- 大量の書き換え直後の遅延（3.3）は、どちらの設定でも変わりません。これは git が内容を比べるためです

見直す条件: 未追跡のファイルが多いフォルダや、`list_changes` が 1 秒を超える実機の報告が出た場合に、`core.untrackedCache`（daemon を伴わない）から試すことを提案します。

### 5.2 目標に無いが、改善を検討すべき箇所

| 箇所 | 実測 | 提案 |
| --- | --- | --- |
| 初回保存（1 万ファイル） | 430 秒 | 内訳は未調査。`git add -A` を単独で実行すると約 7ms/ファイル（2,000 ファイルで 14.7 秒）で、保存 1 回分の 43ms/ファイルの 1/6 です。保存の手順（復元点の作成、サイズ検査、`add -A`、`commit`）のどこが長いかを、先に調べることを提案します。進行状況の表示（件数・経過時間）も検討に値します。なお、`core.fsync=none` を付けても 2,000 ファイルで 14.7 秒 → 12.7 秒で、fsync は主因ではありませんでした |
| 履歴 1 ページ（100 件） | 約 4 秒 | 保存 1 件ごとの `diff --numstat` を、1 回の `log --numstat`（または `--shortstat`）にまとめる。または、最初のページを 20〜30 件にする。実装の変更になるため、判断を仰ぎます |
| 大量の書き換え後の保存・自動保存 | 500MB で 約 22〜31 秒 | 書き換えたデータ量に比例する。自動保存は静止後に裏で動くため、利用者の待ち時間にはなりにくい。保存中の進行状況の表示を検討する |
| 自動保存（同じ内容で呼んだとき） | 1 万ファイルで 1.6 秒、500MB で 4.7 秒 | 変更が無いときの空振りの時間。実際の呼び出し頻度（変更検知後の静止ごと）では問題になりにくい |

### 5.3 設計書 1.4 の記述について

「`git status` の untracked キャッシュ利用を前提」の部分は、今回の計測では前提にしなくても目標を満たしたため、「untracked キャッシュは前提としない」に直すことを提案します（設計書 1.4 は今回、この趣旨で更新しました）。

## 6. 測れなかった項目と理由

| 項目 | 理由 |
| --- | --- |
| 合計 5GB のリポジトリ | 作成と各計測に、ディスクと時間を使いすぎるため。500MB（100 ファイル × 5MB）で代表させた。500MB の初回保存が 23 秒、全書き換え後の保存が 31 秒で、5GB では 10 倍（4〜5 分）になる見込み。5GB 全体の書き換えは、状態表示が 10 秒前後かかる見込み（比例の外挿であり、実測ではない） |
| コールドスタート | 4.2 のとおり |
| 実アプリ（Tauri の画面）経由の状態表示 | `core-ops` の関数の所要時間を測った。IPC・描画・TanStack Query の時間は含まない |
| ファイル監視と自動保存の負荷（実機のファイル変更を伴うもの） | 実際の OS のファイル監視を介した計測はしていない（`app` 層のため。12 章） |
| macOS | この環境は Windows のみ |
| ウイルス対策ソフトの影響 | 有効・無効を切り替えての比較はしていない。Windows の標準の保護が有効な場合、ファイルの作成・読み取りが遅くなる |
| 初回保存の内訳 | 4 手順のどこが長いかのプロファイルは取っていない |
| 実行間の揺れ | 各項目は 3〜5 回の中央値。実行全体の再現（複数回の通し）は 1 回ずつ |

## 付録: 計測の生ログ

`PERF|場面|項目|値` の行です（`cargo test` の出力から抽出。折り返し・順序は原文のまま）。

### A. 1 万ファイル（`HIKAE_PERF_FILES=10000 HIKAE_PERF_HISTORY=0`）

```text
PERF|1万ファイル|git|git version 2.56.0.windows.2 / 入手元=Bundled
PERF|1万ファイル|ファイル数|10000
PERF|1万ファイル|参考/ファイル生成|36.4s
PERF|1万ファイル|(a) 初回保存|430.2s
PERF|1万ファイル|参考/保存データの大きさ(count-objects)|23MiB
PERF|1万ファイル|基準/変更なし/list_changes 1回目|75ms
PERF|1万ファイル|基準/変更なし/list_changes|min=75ms median=75ms max=84ms n=5
PERF|1万ファイル|基準/変更なし/status(ロックを許す)|min=66ms median=68ms max=72ms n=5
PERF|1万ファイル|基準/100件変更/list_changes 1回目|199ms
PERF|1万ファイル|基準/100件変更/list_changes|min=126ms median=136ms max=199ms n=5
PERF|1万ファイル|基準/100件変更/status(ロックを許す)|min=107ms median=114ms max=115ms n=5
PERF|1万ファイル|(d) 自動保存 作成|3.0s
PERF|1万ファイル|(d) 自動保存 同じ内容(作らない)|1.6s (Unchanged)
PERF|1万ファイル|(e) project_status相当/100件変更|min=445ms median=474ms max=691ms n=3
PERF|1万ファイル|(e) project_status相当/変更なし|min=308ms median=329ms max=342ms n=3
PERF|1万ファイル|参考/100件変更の保存(基準)|4.8s
PERF|1万ファイル|untrackedCache/変更なし/list_changes 1回目|75ms
PERF|1万ファイル|untrackedCache/変更なし/list_changes|min=75ms median=75ms max=85ms n=5
PERF|1万ファイル|untrackedCache/変更なし/status(ロックを許す)|min=66ms median=67ms max=69ms n=5
PERF|1万ファイル|untrackedCache/100件変更/list_changes 1回目|258ms
PERF|1万ファイル|untrackedCache/100件変更/list_changes|min=125ms median=135ms max=258ms n=5
PERF|1万ファイル|untrackedCache/100件変更/status(ロックを許す)|min=109ms median=111ms max=113ms n=5
PERF|1万ファイル|参考/100件変更の保存(untrackedCache)|4.4s
PERF|1万ファイル|参考/fsmonitor 有効化後の最初の2回のstatus|0.2s
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes 1回目|53ms
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes|min=52ms median=53ms max=63ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/status(ロックを許す)|min=41ms median=43ms max=51ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes 1回目|198ms
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes|min=94ms median=105ms max=198ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/status(ロックを許す)|min=87ms median=91ms max=100ms n=5
PERF|1万ファイル|参考/100件変更の保存(fsmonitor)|4.5s
PERF|1万ファイル|(f) 履歴 保存の総数|5
PERF|1万ファイル|(f) 履歴 1ページ目の件数|5
PERF|1万ファイル|(f) 履歴 offset=0 limit=100|min=520ms median=565ms max=1516ms n=3
PERF|1万ファイル|(f) 履歴 offset=100 limit=100|min=214ms median=288ms max=322ms n=3
```

### B. 履歴用（`HIKAE_PERF_FILES=200 HIKAE_PERF_HISTORY=130`。場面名は「1万ファイル」と出るが、ファイル数は 200）

```text
PERF|1万ファイル|git|git version 2.56.0.windows.2 / 入手元=Bundled
PERF|1万ファイル|ファイル数|200
PERF|1万ファイル|参考/ファイル生成|0.3s
PERF|1万ファイル|(a) 初回保存|6.5s
PERF|1万ファイル|参考/保存データの大きさ(count-objects)|1MiB
PERF|1万ファイル|基準/変更なし/list_changes 1回目|47ms
PERF|1万ファイル|基準/変更なし/list_changes|min=43ms median=48ms max=62ms n=5
PERF|1万ファイル|基準/変更なし/status(ロックを許す)|min=36ms median=39ms max=46ms n=5
PERF|1万ファイル|基準/100件変更/list_changes 1回目|243ms
PERF|1万ファイル|基準/100件変更/list_changes|min=66ms median=75ms max=243ms n=5
PERF|1万ファイル|基準/100件変更/status(ロックを許す)|min=39ms median=40ms max=44ms n=5
PERF|1万ファイル|(d) 自動保存 作成|2.0s
PERF|1万ファイル|(d) 自動保存 同じ内容(作らない)|0.7s (Unchanged)
PERF|1万ファイル|(e) project_status相当/100件変更|min=236ms median=243ms max=252ms n=3
PERF|1万ファイル|(e) project_status相当/変更なし|min=221ms median=246ms max=257ms n=3
PERF|1万ファイル|参考/100件変更の保存(基準)|4.4s
PERF|1万ファイル|untrackedCache/変更なし/list_changes 1回目|44ms
PERF|1万ファイル|untrackedCache/変更なし/list_changes|min=44ms median=58ms max=71ms n=5
PERF|1万ファイル|untrackedCache/変更なし/status(ロックを許す)|min=31ms median=35ms max=65ms n=5
PERF|1万ファイル|untrackedCache/100件変更/list_changes 1回目|96ms
PERF|1万ファイル|untrackedCache/100件変更/list_changes|min=54ms median=73ms max=96ms n=5
PERF|1万ファイル|untrackedCache/100件変更/status(ロックを許す)|min=43ms median=52ms max=55ms n=5
PERF|1万ファイル|参考/100件変更の保存(untrackedCache)|4.8s
PERF|1万ファイル|参考/fsmonitor 有効化後の最初の2回のstatus|0.2s
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes 1回目|43ms
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes|min=36ms median=43ms max=45ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/status(ロックを許す)|min=28ms median=34ms max=37ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes 1回目|174ms
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes|min=43ms median=53ms max=174ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/status(ロックを許す)|min=35ms median=37ms max=43ms n=5
PERF|1万ファイル|参考/100件変更の保存(fsmonitor)|5.0s
PERF|1万ファイル|参考/履歴用の保存を重ねた時間|130 件 185.6s
PERF|1万ファイル|(f) 履歴 保存の総数|135
PERF|1万ファイル|(f) 履歴 1ページ目の件数|100
PERF|1万ファイル|(f) 履歴 offset=0 limit=100|min=3970ms median=4010ms max=6238ms n=3
PERF|1万ファイル|(f) 履歴 offset=100 limit=100|min=1578ms median=1684ms max=2473ms n=3
```

### C. 500MB 相当（`HIKAE_PERF_LARGE_FILES=100 HIKAE_PERF_LARGE_MB=5`。場面名は「5GB相当」と出るが、合計は 500MB）

```text
PERF|5GB相当|git|git version 2.56.0.windows.2 / 入手元=Bundled
PERF|5GB相当|ファイル数|100
PERF|5GB相当|参考/ファイル生成|1.7s
PERF|5GB相当|(a) 初回保存|23.3s
PERF|5GB相当|参考/保存データの大きさ(count-objects)|500MiB
PERF|5GB相当|基準/変更なし/list_changes 1回目|48ms
PERF|5GB相当|基準/変更なし/list_changes|min=43ms median=48ms max=54ms n=5
PERF|5GB相当|基準/変更なし/status(ロックを許す)|min=27ms median=28ms max=29ms n=5
PERF|5GB相当|基準/100件変更/list_changes 1回目|1005ms
PERF|5GB相当|基準/100件変更/list_changes|min=1005ms median=1015ms max=1034ms n=5
PERF|5GB相当|基準/100件変更/status(ロックを許す)|min=996ms median=1012ms max=1099ms n=5
PERF|5GB相当|(d) 自動保存 作成|21.7s
PERF|5GB相当|(d) 自動保存 同じ内容(作らない)|4.7s (Unchanged)
PERF|5GB相当|(e) project_status相当/100件変更|min=3135ms median=3156ms max=3174ms n=3
PERF|5GB相当|(e) project_status相当/変更なし|min=203ms median=206ms max=214ms n=3
PERF|5GB相当|参考/100件変更の保存(基準)|30.8s
PERF|5GB相当|untrackedCache/変更なし/list_changes 1回目|44ms
PERF|5GB相当|untrackedCache/変更なし/list_changes|min=43ms median=44ms max=52ms n=5
PERF|5GB相当|untrackedCache/変更なし/status(ロックを許す)|min=28ms median=31ms max=34ms n=5
PERF|5GB相当|untrackedCache/100件変更/list_changes 1回目|1028ms
PERF|5GB相当|untrackedCache/100件変更/list_changes|min=1028ms median=1039ms max=1047ms n=5
PERF|5GB相当|untrackedCache/100件変更/status(ロックを許す)|min=1007ms median=1010ms max=1020ms n=5
PERF|5GB相当|参考/100件変更の保存(untrackedCache)|27.3s
PERF|5GB相当|参考/fsmonitor 有効化後の最初の2回のstatus|0.2s
PERF|5GB相当|fsmonitor+untrackedCache/変更なし/list_changes 1回目|44ms
PERF|5GB相当|fsmonitor+untrackedCache/変更なし/list_changes|min=44ms median=44ms max=45ms n=5
PERF|5GB相当|fsmonitor+untrackedCache/変更なし/status(ロックを許す)|min=26ms median=28ms max=31ms n=5
PERF|5GB相当|fsmonitor+untrackedCache/100件変更/list_changes 1回目|1019ms
PERF|5GB相当|fsmonitor+untrackedCache/100件変更/list_changes|min=1007ms median=1019ms max=1210ms n=5
PERF|5GB相当|fsmonitor+untrackedCache/100件変更/status(ロックを許す)|min=1006ms median=1405ms max=1788ms n=5
PERF|5GB相当|参考/100件変更の保存(fsmonitor)|35.5s
PERF|5GB相当|(f) 履歴 保存の総数|5
PERF|5GB相当|(f) 履歴 1ページ目の件数|5
PERF|5GB相当|(f) 履歴 offset=0 limit=100|min=3458ms median=3725ms max=4518ms n=3
PERF|5GB相当|(f) 履歴 offset=100 limit=100|min=195ms median=214ms max=237ms n=3
```

### D. 試走（`HIKAE_PERF_FILES=1000 HIKAE_PERF_HISTORY=5`。場面名は「1万ファイル」と出るが、ファイル数は 1,000）

```text
PERF|1万ファイル|git|git version 2.56.0.windows.2 / 入手元=Bundled
PERF|1万ファイル|ファイル数|1000
PERF|1万ファイル|参考/ファイル生成|0.6s
PERF|1万ファイル|(a) 初回保存|18.5s
PERF|1万ファイル|参考/保存データの大きさ(count-objects)|2MiB
PERF|1万ファイル|基準/変更なし/list_changes 1回目|65ms
PERF|1万ファイル|基準/変更なし/list_changes|min=63ms median=65ms max=74ms n=5
PERF|1万ファイル|基準/変更なし/status(ロックを許す)|min=49ms median=50ms max=52ms n=5
PERF|1万ファイル|基準/100件変更/list_changes 1回目|299ms
PERF|1万ファイル|基準/100件変更/list_changes|min=73ms median=74ms max=299ms n=5
PERF|1万ファイル|基準/100件変更/status(ロックを許す)|min=61ms median=63ms max=63ms n=5
PERF|1万ファイル|(d) 自動保存 作成|1.8s
PERF|1万ファイル|(d) 自動保存 同じ内容(作らない)|1.0s (Unchanged)
PERF|1万ファイル|(e) project_status相当/100件変更|min=296ms median=299ms max=316ms n=3
PERF|1万ファイル|(e) project_status相当/変更なし|min=265ms median=265ms max=302ms n=3
PERF|1万ファイル|参考/100件変更の保存(基準)|4.3s
PERF|1万ファイル|untrackedCache/変更なし/list_changes 1回目|75ms
PERF|1万ファイル|untrackedCache/変更なし/list_changes|min=63ms median=73ms max=83ms n=5
PERF|1万ファイル|untrackedCache/変更なし/status(ロックを許す)|min=50ms median=52ms max=70ms n=5
PERF|1万ファイル|untrackedCache/100件変更/list_changes 1回目|309ms
PERF|1万ファイル|untrackedCache/100件変更/list_changes|min=73ms median=74ms max=309ms n=5
PERF|1万ファイル|untrackedCache/100件変更/status(ロックを許す)|min=59ms median=60ms max=62ms n=5
PERF|1万ファイル|参考/100件変更の保存(untrackedCache)|3.4s
PERF|1万ファイル|参考/fsmonitor 有効化後の最初の2回のstatus|0.1s
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes 1回目|52ms
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/list_changes|min=52ms median=53ms max=62ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/変更なし/status(ロックを許す)|min=35ms median=36ms max=38ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes 1回目|310ms
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/list_changes|min=62ms median=73ms max=310ms n=5
PERF|1万ファイル|fsmonitor+untrackedCache/100件変更/status(ロックを許す)|min=48ms median=57ms max=65ms n=5
PERF|1万ファイル|参考/100件変更の保存(fsmonitor)|3.2s
PERF|1万ファイル|参考/履歴用の保存を重ねた時間|5 件 8.7s
PERF|1万ファイル|(f) 履歴 保存の総数|10
PERF|1万ファイル|(f) 履歴 1ページ目の件数|10
PERF|1万ファイル|(f) 履歴 offset=0 limit=100|min=591ms median=651ms max=1662ms n=3
PERF|1万ファイル|(f) 履歴 offset=100 limit=100|min=205ms median=216ms max=229ms n=3
```
