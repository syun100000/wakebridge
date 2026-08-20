# WakeBridge 開発ドキュメント

## 目的

WakeBridgeは、Windows Server上で複数拠点のWake-on-LANを管理するRustアプリケーションです。Windowsホスト自身からUDP Broadcastを送信せず、SSH経由でYamaha RTXへ固定された`wol send`コマンドを実行します。

## 技術構成

- Rust stable / MSVC
- Tokio、Axum、Askama、SQLite（rusqlite bundled）、Serde
- Argon2idによるパスワードハッシュ
- AES-256-GCM + Windows DPAPIによるSSH Credential暗号化
- tracingによるログ
- サーバーサイドHTML + Vanilla JavaScript
- Windows Serviceは`windows-service`でSCMへ接続

## ディレクトリ構成

| パス | 役割 |
| --- | --- |
| `src/main.rs` | CLI、foreground起動、ユーザー管理 |
| `src/web.rs` | Axumルート、認証済み画面、CSRF、操作処理 |
| `src/auth.rs` | Argon2id、Session、ログインレート制限 |
| `src/db.rs` | SQLite接続、migration、CRUD、監査・履歴 |
| `src/secrets.rs` | Credential暗号化、DPAPI master key |
| `src/providers/` | Provider traitとYamaha RTX実装 |
| `src/service.rs` | Windows Serviceのinstall/start/stop/status/uninstall |
| `templates/` | Askama HTMLテンプレート（UIは日本語） |
| `src/static/` | CSS、Vanilla JavaScript |
| `deploy/iis/` | IIS URL Rewrite / ARR設定例 |
| `installer/` | ビルド済みrelease binaryを梱包するInno Setup定義と生成スクリプト |
| `docs/` | 開発・操作ドキュメント |

## 開発環境

Visual Studio Developer PowerShell（MSVC C++ workload、Windows SDK）で実行する。

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup component add rustfmt clippy
```

Node.js、React、Dockerは不要です。

## ローカル起動

foreground起動では既定で`127.0.0.1:8787`を使用する。開発用HTTP Cookie確認が必要な場合だけ、開発環境で次を設定する。

```powershell
$env:WAKEBRIDGE_DEV_INSECURE_COOKIE = '1'
.\target\release\wakebridge.exe run
```

本番・IIS構成ではSecure Cookieを有効にし、開発用環境変数を設定しない。

## 実装上の重要事項

### パスワード変更

ログインユーザーは`/account/password`から現在パスワードを再確認し、新パスワードと確認入力を送信する。CSRFトークン、現在パスワード検証、12文字以上のArgon2idハッシュ化、監査イベントを必須とする。平文パスワードはログやDBへ保存しない。

管理者のユーザー管理画面には、他ユーザー向けのリセット機能がある。CLIの`user reset-password`も同じSQLiteデータ領域を使用する。

### UI日本語化

利用者向けの画面、ボタン、入力ラベル、状態、監査操作名は日本語を標準とする。Yamaha、SSH、MAC、IP、WOLなどの技術用語は意味が変わらない範囲で英語表記を残してよい。

### Serviceとパス

`service install`は実行中のバイナリを次へコピーしてService登録する。既存Serviceがある場合は構成を更新するため、更新インストールと手動再登録を冪等に扱える。

- Binary: `C:\Program Files\WakeBridge\wakebridge.exe`
- Data: `C:\ProgramData\WakeBridge\`
- Service: `WakeBridge`、Automatic、`NT AUTHORITY\LocalService`

`service start`と`service stop`は既に目的状態なら成功として扱い、状態遷移を最大30秒待つ。`service uninstall`は停止完了を待ってからServiceを削除し、削除完了を確認する。データディレクトリは削除しない。

Web処理は`AppConfig`のデータ領域を使い、Windows固有のSCM操作は`src/service.rs`へ隔離する。

### Windowsインストーラー

`installer/WakeBridge.iss`は、Rustを含まない配布用セットアップEXEを生成する。`installer/build-installer.ps1`は既に生成済みの`target/release/wakebridge.exe`を一時payloadへコピーしてInno Setupへ渡し、完了後にpayloadを削除する。出力は`dist/WakeBridge-Setup-<version>-x64.exe`とSHA-256チェックサムで、payload、DB、master key、実機CredentialはGitへ含めない。

セットアップは固定パスを使う。インストール前に既存の`WakeBridge` Serviceを停止・削除し、バイナリ更新後にServiceを再登録・起動する。`C:\ProgramData\WakeBridge\`は更新・修復・通常アンインストールで保持する。アンインストーラーの明示選択または`/DELETE_DATA`指定時だけ、Service停止後にデータディレクトリを削除する。

## 検証コマンド

コミット前に必ず実行する。

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
.\installer\build-installer.ps1
```

最低限確認する項目:

- Login / Logout / CSRF拒否
- パスワード変更（現在値不一致、確認不一致、成功）
- Settings保存
- Site登録・接続テスト・Fingerprint Trust
- Device登録・WOL履歴
- Usersの管理者リセット
- Service status APIとGraceful Shutdown
- インストーラーのコンパイル、SHA-256生成、固定配置、Service登録・更新・起動
- アンインストールでデータ保持を選んだ場合の`C:\ProgramData\WakeBridge\`保持
- `/DELETE_DATA`を指定した場合だけのデータディレクトリ削除
- 実機検証時はRouter Host、経路、LAN Interface、SSH Host Keyを推測しない

## SQLite・秘密情報

SQLite migrationは`src/db.rs`の起動処理で適用する。実データは`C:\ProgramData\WakeBridge`に置き、Gitへ含めない。次の情報をコード、テスト出力、監査詳細、READMEへ書かない。

- パスワード、Token、Cookie、Session
- SSH Credential、秘密鍵、master key
- 実機のIP、MAC、Fingerprint、DB

## コミット規則

ルートの`AGENTS.md`に従い、すべてのコミットで本書と`docs/OPERATIONS.ja.md`を作成または更新する。実装変更の理由、影響、検証コマンドを開発ドキュメントへ記録する。

## 2026-08-19 変更記録

- 利用者向け画面の英語ラベル・状態・監査表示を日本語化した。
- `/account/password`に現在パスワード確認付きの本人パスワード変更を追加した。
- パスワード変更をCSRF保護し、Argon2idでハッシュ化し、監査イベントへ記録するようにした。
- `AGENTS.md`、開発ドキュメント、操作ドキュメントを追加した。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets`、`cargo build --release`に成功した。

## 2026-08-20 変更記録

- ビルド済みrelease binaryを梱包するInno Setup定義と`installer/build-installer.ps1`を追加した。
- Serviceのinstall/start/stop/uninstallを冪等化し、更新・削除時に状態遷移の完了を待つようにした。
- アンインストール時のデータ保持・明示削除を設計し、秘密情報をインストーラーへ含めないことを検証対象に追加した。
