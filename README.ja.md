# WakeBridge

WakeBridgeは、複数拠点のWake-on-LANを管理するWindows/macOSネイティブRustサービスです。Windows/macOSホスト自身からUDP Broadcastは送信せず、SSH経由でYamaha RTXの固定コマンド wol send を実行します。任意SSHコマンドAPIはありません。

## 主な機能

- Rust stable MSVC、Tokio、Axum、SQLite、Askama、Vanilla JavaScript
- Sites / Devices / Users / Settings / Wake History / Audit Log
- 将来の拡張を考慮したYamaha RTX Provider trait
- 初回SSH Host Key Fingerprint表示・管理者Trust・以後の変更拒否
- SSH CredentialはSQLite平文保存せず、AES-256-GCM＋Windows DPAPIまたはmacOS専用サービスユーザー所有の0600 master keyで暗号化
- Argon2idパスワード、HttpOnly/SameSite Cookie、CSRF対策、ログインレート制限
- LocalServiceで自動起動するWindows Service、専用ユーザーで自動起動するmacOS launchd
- IIS HTTPS終端から127.0.0.1:8787へReverse Proxy
- UIにビルド版数を表示

開発手順は[開発ドキュメント](docs/DEVELOPMENT.ja.md)、管理者・利用者向けの操作手順は[操作ドキュメント](docs/OPERATIONS.ja.md)を参照してください。

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

生成物は`target\release\wakebridge.exe`です。Service登録後の運用バイナリは`C:\Program Files\WakeBridge\wakebridge.exe`を使用します。

## Windowsインストーラー

配布先ではRustや開発ツールをインストールせず、ビルド済み実行ファイルを梱包したセットアップEXEを実行します。開発側でrelease build後、Inno Setupのコンパイラを使って生成します。

~~~powershell
cargo build --release
.\installer\build-installer.ps1
~~~

生成物は`dist\WakeBridge-Setup-<version>-x64.exe`とSHA-256チェックサムです。`build-installer.ps1`は`target\release\wakebridge.exe`を入力として使い、DB、Credential、master keyを梱包しません。Inno Setupはビルド環境だけに必要です。

セットアップはUAC管理者権限で、`C:\Program Files\WakeBridge\`へバイナリを配置し、`WakeBridge` ServiceをLocalService・Automaticで登録して起動します。IIS、Firewall、RTX設定は変更しません。新規環境ではセットアップ後に初期adminをCLIで作成してください。

既存環境の更新ではServiceを停止してバイナリを置き換えます。`C:\ProgramData\WakeBridge\`は保持されるため、ユーザー、パスワード、設定、Credential、master keyは維持されます。

アンインストール時はデータを削除するか確認します。「いいえ」ならServiceとプログラムだけを削除します。「はい」または次の明示指定時だけデータも削除します。

~~~powershell
& 'C:\Program Files\WakeBridge\unins000.exe' /DELETE_DATA
~~~

データ削除にはSQLite、ユーザー、パスワードハッシュ、Credential、master keyが含まれ、元に戻せません。通常の更新・修復ではデータ削除は行いません。

## macOSインストーラー

macOS上でrelease binaryを作成し、Apple標準の`pkgbuild`と`productbuild`でpkg、`hdiutil`でDMGを生成します。

```bash
rustup target add $(rustc -vV | awk '/host:/ {print $2}')
cargo build --release
chmod +x installer/macos/build-installer.sh
installer/macos/build-installer.sh
```

生成物は`dist/WakeBridge-Setup-<version>-macos-<arch>.pkg`、DMGおよびSHA-256チェックサムです。配布先にRust、Node.js、React、Dockerは不要です。

- Binary: `/usr/local/libexec/WakeBridge/wakebridge`
- Data・SQLite・master key: `/Library/Application Support/WakeBridge/`
- Logs: `/Library/Logs/WakeBridge/`
- launchd: `/Library/LaunchDaemons/com.wakebridge.service.plist`
- Service user: `_wakebridge`

インストール時にlaunchdへ登録・自動起動します。アンインストールは`/Applications/WakeBridge/WakeBridge-Uninstaller.command`から実行し、標準ではデータを保持します。データも削除する場合だけ次を明示します。

```bash
sudo /Applications/WakeBridge/WakeBridge-Uninstaller.command --delete-data
```

macOSのpkg/launchdはmacOS実機でビルド・検証してください。Windows環境ではmacOS成果物の実機テストは行いません。

## 初回起動

既定値は次のとおりです。

- Listen: 127.0.0.1:8787
- Data: C:\ProgramData\WakeBridge
- Binary: C:\Program Files\WakeBridge\wakebridge.exe

管理者PowerShellで、Service登録はrelease生成物から一度だけ実行します。Service登録時にバイナリが適切な配置先へコピーされます。

~~~powershell
$Release = (Resolve-Path '.\target\release\wakebridge.exe').Path
& $Release service install

$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge user create --username admin --role admin --data-dir $DataDir
& $WakeBridge service start
& $WakeBridge service status
~~~

`user create`で`--password`を省略すると、安全なランダムパスワードが一度だけ表示されます。忘れた場合は、Serviceを停止してから同じ配置先のバイナリで次を実行します。

~~~powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge service stop
& $WakeBridge user reset-password --username admin --data-dir $DataDir
& $WakeBridge service start
~~~

ログイン後は、上部メニューの「パスワード変更」から現在のパスワードを確認して変更できます。管理者は「ユーザー」画面から他ユーザーのパスワードをリセットできます。

ローカルHTTPでの開発試験だけはWAKEBRIDGE_DEV_INSECURE_COOKIE=1を利用できます。IISでHTTPS終端する本番構成では、UIのSecure Cookieを有効にしてください。

## Windows Service

運用操作は必ず配置済みバイナリを使用します。

~~~powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
& $WakeBridge service install
& $WakeBridge service start
& $WakeBridge service status
& $WakeBridge service stop
& $WakeBridge service uninstall
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
