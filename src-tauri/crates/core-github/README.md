# core-github

Hikae の GitHub OAuth 認証と API 統合モジュール。Device Flow を使用してトークンを取得し、OS キーチェーンで管理します。

## 概要

- **GitHub OAuth Device Flow**: ユーザーがコマンドラインで `client_id` を入力することなく認証できます
- **Token Storage**: トークンは OS のキーチェーン（Windows Credential Manager / macOS Keychain）に安全に保存されます
- **API Integration**: GitHub REST API でユーザー情報を取得し、noreply メールアドレスを生成します
- **Credential Helper**: Git がトークンを使用するための credential helper 出力を生成します

## セットアップ

### 1. GitHub OAuth App の作成

1. GitHub にログインして [Settings → Developer settings → OAuth Apps](https://github.com/settings/developers) に進みます
2. **New OAuth App** をクリックします
3. 以下の情報を入力します:
   - **Application name**: `Hikae` (任意)
   - **Homepage URL**: `http://localhost` (仮の値でよい)
   - **Authorization callback URL**: `http://localhost` (仮の値でよい)
4. **Enable Device Flow** にチェックを入れます（重要）
5. **Register application** をクリックします
6. **Client ID** をコピーします

### 2. Client ID の環境変数設定

#### PowerShell (Windows)

```powershell
$env:HIKAE_GITHUB_CLIENT_ID = "your_client_id_here"
```

#### Bash (macOS / Linux)

```bash
export HIKAE_GITHUB_CLIENT_ID="your_client_id_here"
```

## 使用方法

### Device Flow で認証

```bash
cargo run -p core-github --example device_flow
```

プログラムが以下の情報を表示します:

1. **Verification URI**: ユーザーが訪問する URL
2. **User Code**: ユーザーが入力するコード

ユーザーが GitHub でコードを入力すると、プログラムが自動的にトークンを取得し、ユーザー情報を表示します。

### トークンを保存

```bash
cargo run -p core-github --example device_flow -- --save
```

トークンが OS キーチェーンに保存されます。

### 保存されたトークンを削除

```bash
cargo run -p core-github --example device_flow -- --delete
```

### 保存状態を確認

```bash
cargo run -p core-github --example device_flow -- --load
```

## テスト

### 通常のテスト

```bash
cargo test -p core-github
```

HTTP をモックする単体テスト、エラーハンドリングテスト、トークンの隠蔽テストが実行されます。

### キーチェーン実保存テスト

キーチェーン操作を伴うテストは CI では実行されないよう `#[ignore]` が付いています。手動実行する場合:

```bash
cargo test -p core-github -- --ignored
```

このテストはマシンの OS キーチェーンに実際にトークンを保存・読み込み・削除します。

### 静的解析

```bash
cargo fmt -p core-github --check
cargo clippy -p core-github --all-targets -- -D warnings
```

## トラブルシューティング

### `device_flow_disabled` エラー

**原因**: OAuth App で Device Flow が無効になっている

**解決方法**:
1. [OAuth Apps 設定](https://github.com/settings/developers) にアクセス
2. 該当するアプリを選択
3. **Enable Device Flow** にチェックを入れて **Update application** をクリック

### Organization のリポジトリが見えない

**原因**: OAuth App が Org にアクセスする権限がない

**解決方法**:
1. GitHub アカウント設定 → [Authorized OAuth Apps](https://github.com/settings/applications) に進みます
2. Hikae を選択
3. 該当する Organization の右側の **Grant** をクリックして承認します

### トークンを取り消したい

**方法**:
1. GitHub アカウント設定 → [Authorized OAuth Apps](https://github.com/settings/applications) に進みます
2. Hikae を選択
3. **Revoke** をクリック

## セキュリティ

### トークン保護

- トークンは平文でログに出されません（`Debug` / `Display` では `***` 表示）
- トークンへのアクセスは明示的なメソッド `expose_secret()` のみ
- `serde::Serialize` は実装されず、誤シリアライズを防止
- エラーメッセージにトークンを含めない

### 保存方法

- OS のネイティブキーチェーンを使用（暗号化保存）
- Windows: Credential Manager
- macOS: Keychain

## 公開 API

### Device Flow

```rust
pub async fn request_device_code(&self) -> Result<DeviceCode, AuthError>
pub async fn poll_for_token(
    &self,
    device: &DeviceCode,
    cancel_token: Option<&CancellationToken>,
) -> Result<AccessToken, AuthError>
pub fn client_id_from_env() -> Result<String, AuthError>
```

### Token Store

```rust
pub fn save(token: &AccessToken) -> Result<(), AuthError>
pub fn load() -> Result<Option<AccessToken>, AuthError>
pub fn delete() -> Result<(), AuthError>
pub fn exists() -> Result<bool, AuthError>
```

### User API

```rust
pub async fn fetch_user(&self, token: &AccessToken) -> Result<User, AuthError>
pub fn noreply_email(&self) -> String
```

### Credential Helper

```rust
pub fn credential_helper_output(host: &str, token: &AccessToken) -> String
```

## 注記

- Credential helper（`!<app> credential` の実装）は app 層で行います
- このモジュールは Tauri に依存しません（CLI でも単体でテスト可能）
