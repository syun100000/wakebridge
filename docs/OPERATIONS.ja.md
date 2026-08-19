# WakeBridge 操作ドキュメント

## 標準配置

- 運用バイナリ: `C:\Program Files\WakeBridge\wakebridge.exe`
- データ・SQLite・master key: `C:\ProgramData\WakeBridge\`
- Web待受: `http://127.0.0.1:8787`
- Service名: `WakeBridge`
- Serviceアカウント: `NT AUTHORITY\LocalService`

IISを使う本番構成では、IISでHTTPSを終端し、ARRで`127.0.0.1:8787`へ転送する。WakeBridgeを直接インターネットへ公開しない。

## 初回インストールとService登録

Visual Studio Developer PowerShellでrelease build後、管理者PowerShellから実行する。

```powershell
$Release = (Resolve-Path '.\target\release\wakebridge.exe').Path
& $Release service install

$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge user create --username admin --role admin --data-dir $DataDir
& $WakeBridge service start
& $WakeBridge service status
```

`user create`で`--password`を省略すると、ランダムパスワードが一度だけ表示される。表示値は安全なパスワード保管場所へ保存し、Git、ログ、チャットへ記録しない。

## Service操作

```powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'

& $WakeBridge service status
& $WakeBridge service start
& $WakeBridge service stop
& $WakeBridge service uninstall
```

`service uninstall`はService登録だけを削除し、`C:\ProgramData\WakeBridge`のDB・Credential・master keyを削除しない。Service削除後はWeb UIが起動しないため、再利用する場合は`service install`と`service start`を実行する。

## CLIでadminパスワードをリセット

現在のパスワードを復元することはできない。忘れた場合は、管理者PowerShellで運用配置済みバイナリと運用データ領域を明示する。

Serviceが起動中の場合:

```powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'

& $WakeBridge service stop
& $WakeBridge user reset-password --username admin --data-dir $DataDir
& $WakeBridge service start
& $WakeBridge service status
```

Serviceを削除済みまたは停止済みの場合:

```powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge user reset-password --username admin --data-dir $DataDir
```

`Generated password (show once): ...`に表示された値が新パスワードである。パスワードを再表示する機能はない。CLIの実行時にパスワード、DB、master keyをログへ出力しない。

補助コマンド:

```powershell
& $WakeBridge user list --data-dir $DataDir
& $WakeBridge user create --username operator --role operator --data-dir $DataDir
```

## UIから自分のパスワードを変更

1. `http://127.0.0.1:8787`またはIISのHTTPS URLを開く。
2. 現在のユーザーでログインする。
3. 上部メニューの「パスワード変更」を開く。
4. 現在のパスワード、新しいパスワード、確認入力を入力する。
5. 「パスワードを変更」を押す。

現在のパスワードが一致しない場合は更新されない。新パスワードは12文字以上で、確認入力との一致が必要である。成功操作は監査ログへ記録する。

## 管理者によるユーザー管理

管理者は「ユーザー」画面でオペレーターまたは管理者を作成できる。既存ユーザーを救済する場合は「パスワードをリセット（管理者）」を使う。通常の本人変更には「パスワード変更」を使う。

オペレーターはデバイス閲覧、WOL実行、履歴閲覧を行える。拠点、ユーザー、設定の変更は管理者だけが行える。

## 拠点の登録

1. 「拠点」→「Yamaha RTX拠点を登録」を開く。
2. 実機確認済みの拠点名、ルーターホスト、SSHポート、LANインターフェース、SSHユーザー名、SSH認証情報を入力する。
3. RTX810で古いSSH方式が必要な場合だけ、「古いSSH方式を許可」を有効にする。
4. 「拠点を登録」を押す。
5. 「接続テスト」で、SSH接続と固定の`show version`確認を実行する。
6. 表示されたSSHホスト鍵Fingerprintを管理者が確認し、「取得したFingerprintを信頼」を押す。

実機のIP、経路、interface、SSHアルゴリズム、Fingerprintを推測しない。Fingerprint変更時は接続拒否として扱い、再Trustする前に実機交換・設定変更の有無を確認する。

## デバイス登録とWOL

1. 「デバイス」→「デバイスを登録」を開く。
2. 拠点、名前、MACアドレス、必要ならIPアドレスと説明を入力する。
3. MACアドレスは正規化・厳格検証される。
4. 「デバイスを登録」後、対象行の「起動（WOL）」を押す。

WakeBridgeはWindowsからUDP Broadcastを送信しない。認証済みSSHでRTXへ接続し、固定されたYamaha形式の`wol send`だけを実行する。重要PCで試験する場合は、事前に対象を明示し、無関係なPCを登録しない。

「WOL送信成功」はRTXへのコマンド処理成功を示すもので、対象PCの起動完了を自動保証するものではない。必要に応じて対象PCの電源状態を別途確認する。

## 履歴・監査

- 「履歴」: WOLの成功・失敗、対象拠点、対象デバイス、メッセージ
- 管理者の「監査ログ」: ログイン、拠点・デバイス・ユーザー・設定変更、Fingerprint Trust、パスワード操作

パスワード、SSH認証情報、Session、master keyは履歴・監査ログへ保存しない。

## 設定

管理者は「設定」でサイトタイトルとCookie設定を変更できる。IIS HTTPS構成ではSecure Cookieを有効にする。HTTPの`WAKEBRIDGE_DEV_INSECURE_COOKIE=1`は開発試験専用であり、本番で使用しない。

## IIS連携

1. IIS URL RewriteとApplication Request Routingを導入する。
2. ARR Proxyを有効化する。
3. `deploy/iis/web.config`をHTTPS IIS Siteへ適用する。
4. 外部HTTPS URLからログイン画面を開く。
5. IISからlocalhost転送されること、CookieがSecure属性であることを確認する。

## 障害対応

### Web UIが開かない

```powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
& $WakeBridge service status
Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort 8787 -State Listen
Invoke-WebRequest http://127.0.0.1:8787/api/health
```

Serviceが未登録なら`service install`、停止中なら`service start`を実行する。IIS経由だけ失敗する場合はWakeBridge、IIS URL Rewrite、ARR Proxy、HTTPS Siteを分けて確認する。

### SSH接続に失敗する

ルーターホスト、TCP/SSHポート、経路、LANインターフェース、SSHユーザー、Credential、Host Key Fingerprintを実機と照合する。任意コマンドで回避しない。RTX810の古いアルゴリズム許可は該当拠点だけに限定する。

### パスワードを忘れた

「CLIでadminパスワードをリセット」または管理者の「パスワードをリセット（管理者）」を使用する。ハッシュから旧パスワードを復元しない。

## バックアップと公開禁止情報

Service停止後、アクセス権を保ったまま`C:\ProgramData\WakeBridge`をバックアップする。次のファイル・値を公開repositoryへ含めない。

- `wakebridge.db`、`wakebridge.db-wal`、`wakebridge.db-shm`
- `master.key.dpapi`
- 実際のIP、MAC、SSH Credential、Fingerprint、Token、秘密鍵

## 2026-08-19 変更記録

- 画面上部メニューの「パスワード変更」から、ログイン中ユーザー自身がパスワードを変更できるようにした。
- 管理者による他ユーザーのリセットと、配置済みバイナリを使うCLIリセット手順を明記した。
- 日本語UIの拠点・デバイス・ユーザー・履歴・設定操作を追加・整理した。
