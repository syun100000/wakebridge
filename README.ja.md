# WakeBridge

WakeBridgeは、複数拠点のWake-on-LANを管理するWindowsネイティブRustサービスです。Windowsホスト自身からUDP Broadcastは送信せず、SSH経由でYamaha RTXの固定コマンド wol send を実行します。任意SSHコマンドAPIはありません。

## 主な機能

- Rust stable MSVC、Tokio、Axum、SQLite、Askama、Vanilla JavaScript
- Sites / Devices / Users / Settings / Wake History / Audit Log
- 将来の拡張を考慮したYamaha RTX Provider trait
- 初回SSH Host Key Fingerprint表示・管理者Trust・以後の変更拒否
- SSH CredentialはSQLite平文保存せず、AES-256-GCM＋Windows DPAPI保護master keyで暗号化
- Argon2idパスワード、HttpOnly/SameSite Cookie、CSRF対策、ログインレート制限
- LocalServiceで自動起動するWindows Service
- IIS HTTPS終端から127.0.0.1:8787へReverse Proxy

## ビルド

MSVC C++ workloadとWindows SDKを含むVisual Studio Developer PowerShellで実行します。

~~~powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup component add rustfmt clippy
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
~~~

生成物はtarget\release\wakebridge.exeです。

## 初回起動

既定値は次のとおりです。

- Listen: 127.0.0.1:8787
- Data: C:\ProgramData\WakeBridge
- Binary: C:\Program Files\WakeBridge\wakebridge.exe

初期adminを作成します。--passwordを省略すると安全なランダム値を一度だけ表示します。

~~~powershell
.\wakebridge.exe user create --username admin --role admin
.\wakebridge.exe run
~~~

ローカルHTTPでの開発試験だけはWAKEBRIDGE_DEV_INSECURE_COOKIE=1を利用できます。IISでHTTPS終端する本番構成では、UIのSecure Cookieを有効にしてください。

## Windows Service

ビルド後、管理者PowerShellで実行します。

~~~powershell
.\wakebridge.exe service install
.\wakebridge.exe user create --username admin --role admin
.\wakebridge.exe service start
.\wakebridge.exe service status
.\wakebridge.exe service stop
.\wakebridge.exe service uninstall
~~~

アンインストールしてもデータディレクトリは削除しません。Service install時にNT AUTHORITY\LocalServiceへデータディレクトリの変更権限を付与し、Automatic Startを設定します。

## IIS連携

IIS URL RewriteとApplication Request Routingを導入し、ARR Proxyを有効化したうえで、HTTPS IIS Siteにdeploy/iis/web.configを適用します。IISがHTTPSを終端し、WakeBridgeへはlocalhostだけで転送します。

## Yamaha RTX接続・Wake手順

1. UIでYamaha RTX Siteを追加します。
2. 実機確認済みのRouter Host、SSH Port、LAN Interface、SSH Username、Credentialを入力します。
3. Test ConnectionでFingerprint取得と固定の読み取り専用show version確認を行います。
4. 表示されたFingerprintを確認し、対象SiteのTrust observed fingerprintを押します。
5. 正規化・厳格検証されるMAC AddressでDeviceを登録します。
6. Wakeを押すと、Providerは次の固定形式だけを生成します。

~~~text
wol send -i 1 -c 3 lan1 02:00:00:00:00:10 192.0.2.10 udp 9
~~~

上記はダミー値です。実IP、MAC、Credential、DB、master keyは公開repositoryへ含めません。

公式資料: https://www.rtpro.yamaha.co.jp/RT/docs/wol/wol.html

## 構成

~~~text
Browser --HTTPS--> IIS/ARR --HTTP localhost--> WakeBridge --SSH--> Yamaha RTX
                                                               |
                                                               +--> wol send --> LAN PC
~~~

## ライセンス

MIT LicenseまたはApache License 2.0のいずれかを選択できます。LICENSE-MITとLICENSE-APACHEを参照してください。
