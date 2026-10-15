# モバイル版の配布（TestFlight）と、あとで行うこと

モバイル版（`hikae-mobile`。設計書 14 章）を、GitHub Actions でビルドして TestFlight に配布するための手順と、実機で確認することのまとめ。ワークフローは `.github/workflows/mobile.yml`。

## 1. 一度だけ行う設定（利用者の作業）

Claude 側ではできない作業（Apple のアカウントでの操作）。上から順に行う。

### 1.1 Apple Developer（https://developer.apple.com/account）

1. **Team ID を控える**: Membership details の「Team ID」（10 文字）。→ `APPLE_DEVELOPMENT_TEAM`
2. **App ID（Identifier）を登録する**: Certificates, Identifiers & Profiles → Identifiers → 追加 → App IDs → App。Bundle ID は **Explicit** で `com.shiki01.hikae.mobile`（`src-tauri/mobile/tauri.conf.json` の `identifier` と同じ）。Capabilities は何も付けない。
3. **配布用の証明書を作る**: Certificates → 追加 → **Apple Distribution**。Mac のキーチェーンアクセスで CSR を作って登録する。作った証明書をキーチェーンに入れ、キーチェーンアクセスから **`.p12` に書き出す**（パスワードを付ける）。→ `IOS_CERTIFICATE`（base64）と `IOS_CERTIFICATE_PASSWORD`
4. **プロビジョニングプロファイルを作る**: Profiles → 追加 → **App Store Connect**（配布用）。手順 2 の App ID と手順 3 の証明書を選び、`.mobileprovision` をダウンロードする。→ `IOS_MOBILE_PROVISION`（base64）。証明書とプロファイルには有効期限（1 年）がある。切れたら作り直して、シークレットを更新する

### 1.2 App Store Connect（https://appstoreconnect.apple.com）

1. **アプリを作る**: マイ App → 追加 → 新規 App。プラットフォーム iOS、名前、主言語（日本語）、Bundle ID は手順 1.1-2 のもの、SKU は任意（例: `hikae-mobile`）。名前は他のアプリと重複できない（「Hikae」が使えなければ別名でよい。後から変えられる）
2. **API キーを作る**（アップロード用）: ユーザとアクセス → 統合 → App Store Connect API → チームキー → 追加。アクセス権は **App Manager**。作ったら次を控える。**`.p8` は 1 回しかダウンロードできない**
   - Key ID → `APPLE_API_KEY_ID`
   - Issuer ID（画面の上の方）→ `APPLE_API_ISSUER`
   - `.p8` ファイル → `APPLE_API_KEY_P8`（base64）
3. **内部テスターを用意する**: TestFlight → 内部テスト → グループを作り、テスターを追加する（テスターは App Store Connect のユーザである必要がある。ユーザとアクセスで招待する）。外部テスト（App Store Connect のユーザでない人）は、初回のビルドで Apple のベータ審査が必要

### 1.3 GitHub のシークレット

リポジトリの Settings → Secrets and variables → Actions → New repository secret に、次の 7 つを登録する。ファイルの値は base64 にする（Mac のターミナルで `base64 -i ファイル名 | pbcopy`）。

| 名前                        | 内容                                           |
| --------------------------- | ---------------------------------------------- |
| `APPLE_DEVELOPMENT_TEAM`    | Team ID                                        |
| `IOS_CERTIFICATE`           | Apple Distribution 証明書の `.p12`（base64）   |
| `IOS_CERTIFICATE_PASSWORD`  | `.p12` を書き出したときのパスワード            |
| `IOS_MOBILE_PROVISION`      | プロビジョニングプロファイル（base64）         |
| `APPLE_API_KEY_ID`          | API キーの Key ID                              |
| `APPLE_API_ISSUER`          | API キーの Issuer ID                           |
| `APPLE_API_KEY_P8`          | API キーの `.p8`（base64）                     |

先頭の 4 つが揃うと、署名して ipa を作る（artifact に出る）。**署名して ipa を作る手順と TestFlight へのアップロードは、シークレットが無いため CI では一度も実行していない**（署名なしの iOS ビルドと Android の APK は CI で成功を確認済み）。最初の実行で失敗したら、Actions のログを見て直す（第 5 章）。残りの 3 つも揃うと、TestFlight へアップロードする。**何も登録しなくても、ワークフローは動く**（iOS は Rust のコンパイル確認だけを行い、Android は APK を作る）。

## 2. ビルドと配布の流れ

- **TestFlight へ出す**: GitHub の Actions → Mobile → Run workflow（`testflight` にチェック）。または `main` への push（モバイルに関係するファイルの変更があるときだけ動く）。
- ビルド番号（`CFBundleVersion`）は GitHub Actions の実行番号を使う。同じ版（`tauri.conf.json` の `version`）の中で増え続ける必要があるため。版を上げるときは `src-tauri/mobile/tauri.conf.json` の `version` を変える。
- アップロード後、App Store Connect の TestFlight に出るまで数分〜30 分かかる（「処理中」の間は選べない）。メールで「処理が完了した」知らせが届く。
- 内部テスターのグループに、ビルドを追加する（グループの設定で「自動で配布」にすると手間が省ける）。テスターは iPhone の TestFlight アプリから入れる。ビルドは 90 日で期限が切れる。
- 輸出コンプライアンスの質問は、`Info.ios.plist` の `ITSAppUsesNonExemptEncryption = false` で出ない（通信は OS 標準の HTTPS だけ）。
- Android: Actions の artifact（`hikae-android-debug-apk`）の APK を、端末に入れて試す（`adb install`、または端末にファイルを送って開く。提供元不明のアプリの許可が要る）。Google Play への配布は未対応（リリース署名の鍵とストアの登録が要る）。

## 3. 実機で確認すること（TestFlight で入れたあと）

自動テストでは確認できなかったもの。上から順に。

### 3.1 最初に（ここが通らないと先へ進めない）

- [ ] アプリが起動し、ログイン画面が出る
- [ ] 「GitHub を開く」で Safari が開き、確認コードを入力できる。**確認ページから戻ったあと、ログインが完了する**（アプリが中断された間の通信の失敗は、再試行する作りにしたが未確認）
- [ ] 保存先（リポジトリ）の一覧が出る。非公開のリポジトリと Organization のものも出る
- [ ] アプリを終了して開き直しても、ログインしたまま（トークンが Keychain に残る）

### 3.2 閲覧

- [ ] フォルダを開ける。日本語名のファイル・フォルダが正しく出る
- [ ] テキスト、画像（png・jpeg・webp・svg）、PDF、docx、xlsx、pptx が開く。**日本語の PDF が文字化けしない**（pdf.js が外部の文字情報を取れないため、出ないことがある。設計書 11.7）
- [ ] 大きいファイル（25 MB 超）で「大きすぎて開けません」と出る
- [ ] 「過去の版」を開ける。古い内容が見える
- [ ] 「ダウンロード」で共有シートが出て、「ファイルに保存」できる。出ない場合は設計書 14.7 のフォールバック（`<a download>`）が動くか確かめる

### 3.3 アップロード

- [ ] 「ファイルを選ぶ」（ファイル App）、「写真を選ぶ」（写真ライブラリ。撮影も）で項目が増える
- [ ] 「テキストを入力」と「クリップボードから貼り付け」（文字と画像）が動く。貼り付けの許可のダイアログが出ることがある
- [ ] 「追加して保存」で、**本物のリポジトリに 1 件のコミットとして入る**（GitHub の画面で確認。作者が自分、メールが noreply になっていること）
- [ ] 同名のファイルがあるとき、3 択（置き換える・両方残す・追加しない）が効く。「置き換える」のあと、古い版が履歴に残っている
- [ ] デスクトップ版（`hikae`）で取り込み、同じファイルが入る（文字化けや改行の変化がない）
- [ ] 別の端末で先に更新があったとき、「他の場所で更新されたため追加できませんでした」と出て、もう一度で成功する
- [ ] 機内モードで操作して、エラーの 3 要素（何が起きたか・データは無事か・次の行動）が出る

### 3.4 見た目と操作

- [ ] ノッチ・ホームインジケーターに重ならない（画面の上端と下端の余白を `env(safe-area-inset-*)` で入れた）
- [ ] 小さい iPhone（幅 320〜375px）と大きい iPhone で崩れない。iPad は未検討（レイアウトは 1 カラムのまま広がる）
- [ ] ダークモード。タップ領域の大きさ。VoiceOver での読み上げ
- [ ] Master CSS の `flex-wrap:wrap` が効いているか（既存コードと同じ書き方で、生成されない可能性がある）
- [ ] 画像の表示（`data:` URI）が CSP で止められていない

### 3.5 Android（APK を入れたとき）

- [ ] ログインでき、**アプリを閉じて開き直してもログインが残る**（Android Keystore を使うプラグイン `tauri-plugin-hikae-secure-store`。実機で一度も動かしていない）
- [ ] 3.2〜3.4 の項目。特にファイル選択、共有（ダウンロード）、クリップボード

## 4. まだ決まっていない・未実装のこと

- 署名なしの確認としての `cargo build --target aarch64-apple-ios` は CI で行うが、**ストアの審査（外部テスト・App Store 公開）に必要なもの**（プライバシーポリシーの URL、スクリーンショット、年齢制限の回答、アプリの説明）は未用意。内部テストだけなら不要
- アプリのアイコンは、デスクトップ版のものを使っている（iOS 用に透明を白で埋めた）。デザインが決まったら `pnpm tauri:mobile icon <1024px の画像> -o src-tauri/mobile/icons` で作り直し、`icons/ios/` の PNG は透明を含まないこと（App Store の要件）を確かめる
- Android のリリース署名（キーストア）と AAB。Google Play に出すときに設定する
- 1 ファイル 25 MiB・1 回 60 MiB の上限は暫定値（設計書 14.5）。実機で時間とメモリを見て決め直す
- iPad の画面と、横向き
- アプリの内部の版の表示（設定画面に版を出していない）
- `tauri ios init` / `android init` の生成物（`gen/`）は、CI が毎回作る。手元でも触るときは `pnpm tauri:mobile ios init`（Xcode が要る）。生成物に手を入れる必要が出たら、`gen/` をコミットする運用に切り替える

## 5. うまくいかないとき

| 症状                                                | 確認すること                                                                                                         |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| iOS のジョブで署名が失敗する                         | プロビジョニングプロファイルの Bundle ID・証明書が合っているか、期限が切れていないか、`.p12` のパスワードが合っているか |
| アップロードで「ビルド番号が重複」                   | 同じ版・同じ実行番号は再アップロードできない。ワークフローを新しく実行する（実行番号が増える）                           |
| `altool` で認証エラー                                | `APPLE_API_KEY_P8` が base64 になっているか、Key ID と Issuer ID が合っているか、キーの権限が App Manager か           |
| TestFlight に出てこない                              | App Store Connect のメールを確認する（無効なバイナリのとき、理由が届く）。アイコンの透明、`Info.plist` の不足が多い   |
| Android のジョブが失敗する                           | `gen/android` の生成（`android init`）のログ、NDK の版（`NDK_VERSION`）、`platforms;android-36` の有無                |
