# モバイル版の配布（TestFlight）と、利用者が行うこと

CI で確認できるものは確認済みです（14.11）。このページには、CI では確認できず、利用者の操作が必要なものだけを残しています。

## 1. 一度だけ行う設定（約 15 分）

1. App Store Connect の「ユーザとアクセス」→「統合」→「App Store Connect API」で、API キーを作成します（アクセス権は「管理」）。`.p8` をダウンロードし、Key ID と Issuer ID を控えます。
2. App Store Connect の「マイ App」→「新規 App」で、アプリを作成します（バンドル ID `com.shiki01.hikae.mobile`。API では作成できないため手作業です。バンドル ID が選択肢に無い場合は手順 3 を先に行います）。
3. Mac で、gh にログイン済みの状態から次を実行します。App ID・配布証明書・プロビジョニングプロファイルを作り、GitHub のシークレット 7 件（`IOS_CERTIFICATE`、`IOS_CERTIFICATE_PASSWORD`、`IOS_MOBILE_PROVISION`、`APPLE_DEVELOPMENT_TEAM`、`APPLE_API_KEY_ID`、`APPLE_API_ISSUER`、`APPLE_API_KEY_P8`）を登録します。

```sh
node scripts/setup-ios-signing.mjs --key-id <Key ID> --issuer <Issuer ID> --p8 <AuthKey.p8 のパス> --set-github-secrets
```

4. GitHub の Actions →「Mobile」→「Run workflow」で `testflight` を選びます。ビルドが終わると TestFlight へ自動でアップロードされます。
5. App Store Connect の TestFlight で、内部テスターを追加します（輸出コンプライアンスは Info.plist で回答済みです）。

## 2. 実機で確認すること（CI では確認できないもの）

| 項目 | 内容 |
| --- | --- |
| ログイン | 「GitHub でログイン」→ ブラウザでコード入力 → アプリへ戻って、アカウントが表示される（Device Flow の往復。コード取得までは Android エミュレータの CI で確認済み） |
| 閲覧 | 実際のリポジトリの一覧・フォルダ・テキスト・画像・PDF（日本語のフォントを含む）・docx・xlsx・pptx の表示 |
| アップロード | ファイルの選択、写真、テキスト、クリップボードを追加して「アップロード」。デスクトップ版で取り込んで反映される |
| ダウンロード | 共有シートからの保存 |
| 見た目 | ノッチ・ホームインジケータ周辺（セーフエリア）、ダークモード、横向き |
| iOS の Keychain | 署名つきの実機ビルドで、ログイン後にアプリを再起動してもログイン状態が残る（シミュレータの署名なしビルドでは確認できないため。CI のログに自己確認の結果を情報として出している） |
| 署名つき ipa と TestFlight | 手順 1 の 4 が通ること |

実際の GitHub に対する閲覧・アップロードの確認は、手元で次の 1 コマンドでも行えます（捨ててよい検証用リポジトリを指定します）。

```sh
GITHUB_TOKEN=<トークン> cargo run -p core-remote --example live_smoke -- <owner>/<検証用リポジトリ>
```

## 3. CI で確認済みのもの

- Android: APK のビルド、エミュレータでの起動とログイン画面、実際の GitHub への HTTPS 接続（Device Flow のコード取得）、Android Keystore の保管（7 件のテスト）、クラッシュなし。
- iOS: 署名なしのシミュレータ向けビルド、起動して 40 秒間落ちないこと、`core-github`・`core-remote` のテスト（シミュレータ上）。
- 共通: GitHub を模したサーバーに対する閲覧・アップロードのテスト（fast-forward のみ・force を送らない・422 の再試行）、画面の e2e（モック API）。

## 4. うまくいかないとき

- `setup-ios-signing.mjs` が失敗する: API キーのアクセス権が「管理」か確認します。証明書の上限（配布証明書は 3 件まで）に達している場合は、不要な証明書を失効させます。
- ビルドの署名で失敗する: `APPLE_DEVELOPMENT_TEAM` と、プロファイルのバンドル ID が一致しているか確認します。
- アップロードで失敗する: App Store Connect にアプリの作成（手順 2）が済んでいるか確認します。
