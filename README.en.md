# WakeBridge

WakeBridge is a native Windows/macOS Rust service for managing Wake-on-LAN across multiple sites. It sends a fixed Yamaha RTX wol send command over SSH; WakeBridge never broadcasts UDP directly from the host and exposes no arbitrary SSH command API.

## Features

- Rust stable MSVC binary, Tokio, Axum, SQLite, Askama templates, and vanilla JavaScript
- Sites, devices, users, settings, wake history, and audit history
- Yamaha RTX provider behind a provider trait
- SSH host-key fingerprint trust on first connection and strict mismatch rejection afterwards
- SSH credentials encrypted with AES-256-GCM under a machine-protected, installation-specific DPAPI master key
- On macOS, the installation-specific master key is a 0600 file owned by the dedicated service user
- Argon2id passwords, in-memory sessions, HttpOnly/SameSite cookies, CSRF tokens, and login throttling
- Windows Service commands with LocalService and automatic start
- macOS launchd service with a dedicated service user
- IIS reverse-proxy configuration for 127.0.0.1:8787
- Build version displayed in the web UI

See the [development documentation](docs/DEVELOPMENT.ja.md) and [operations documentation](docs/OPERATIONS.ja.md) for the complete procedures. The Japanese README is the default repository landing page; this file contains the English overview.

## Build

Open a Visual Studio Developer PowerShell with the MSVC C++ workload and Windows SDK installed:

~~~powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup component add rustfmt clippy
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
~~~

The build output is `target\release\wakebridge.exe`. After service installation, use the deployed binary at `C:\Program Files\WakeBridge\wakebridge.exe`.

## Windows installer

Distribution uses a normal setup EXE containing the already-built release binary. Rust, Node.js, Docker, and Inno Setup are not required on the target server. On the build machine:

~~~powershell
cargo build --release
.\installer\build-installer.ps1
~~~

The output is `dist\WakeBridge-Setup-<version>-x64.exe` plus a SHA-256 checksum. The packaging script consumes `target\release\wakebridge.exe` and never packages a database, credential, or master key. Inno Setup is required only on the build machine.

The setup requests administrator approval, installs the binary under `C:\Program Files\WakeBridge\`, registers `WakeBridge` as an Automatic LocalService, and starts it. It does not change IIS, firewall rules, or RTX configuration. A fresh data directory still requires creating the initial admin with the CLI.

Upgrades stop the service and replace the binary while preserving `C:\ProgramData\WakeBridge\`; users, password hashes, settings, credentials, and the master key remain unchanged.

The uninstaller asks whether to delete application data. Choosing No removes only the service and program files. Choosing Yes, or explicitly passing the following switch, also removes the data directory:

~~~powershell
& 'C:\Program Files\WakeBridge\unins000.exe' /DELETE_DATA
~~~

Data deletion includes SQLite, users, password hashes, credentials, and the master key and cannot be undone. Normal upgrade and repair never delete the data directory.

## macOS installer

On macOS, build the release binary and package it with Apple's `pkgbuild`, `productbuild`, and `hdiutil` tools:

```bash
rustup target add $(rustc -vV | awk '/host:/ {print $2}')
cargo build --release
chmod +x installer/macos/build-installer.sh
installer/macos/build-installer.sh
```

The output is `dist/WakeBridge-Setup-<version>-macos-<arch>.pkg`, a DMG, and SHA-256 checksums. Rust, Node.js, React, and Docker are not required on the target Mac.

- binary: `/usr/local/libexec/WakeBridge/wakebridge`
- data, SQLite, and master key: `/Library/Application Support/WakeBridge/`
- logs: `/Library/Logs/WakeBridge/`
- launchd plist: `/Library/LaunchDaemons/com.wakebridge.service.plist`
- service user: `_wakebridge`

Installation registers and starts launchd automatically. Run `/Applications/WakeBridge/WakeBridge-Uninstaller.command` to uninstall; data is preserved by default. To explicitly delete data:

```bash
sudo /Applications/WakeBridge/WakeBridge-Uninstaller.command --delete-data
```

Build and test the macOS pkg and launchd behavior on a real Mac. Windows cannot perform the macOS runtime verification.

## First run

The default runtime is:

- listen: 127.0.0.1:8787
- data: C:\ProgramData\WakeBridge
- binary: C:\Program Files\WakeBridge\wakebridge.exe

For the initial service installation, run the release binary once. Installation copies it to the deployment path:

~~~powershell
$Release = (Resolve-Path '.\target\release\wakebridge.exe').Path
& $Release service install

$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge user create --username admin --role admin --data-dir $DataDir
& $WakeBridge service start
& $WakeBridge service status
~~~

If `--password` is omitted, a random password is printed once. If it is lost, stop the service and reset it with the deployed binary:

~~~powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
$DataDir = 'C:\ProgramData\WakeBridge'
& $WakeBridge service stop
& $WakeBridge user reset-password --username admin --data-dir $DataDir
& $WakeBridge service start
~~~

After login, users can change their own password from the "パスワード変更" account page. Administrators can reset another user from the Users page.

For local HTTP testing only, set WAKEBRIDGE_DEV_INSECURE_COOKIE=1. Keep the database setting Secure Cookie enabled when IIS terminates HTTPS.

## Windows Service

Use the deployed binary for service operations:

~~~powershell
$WakeBridge = 'C:\Program Files\WakeBridge\wakebridge.exe'
& $WakeBridge service install
& $WakeBridge service start
& $WakeBridge service status
& $WakeBridge service stop
& $WakeBridge service uninstall
~~~

Service installation preserves the data directory when uninstalled. The installer grants NT AUTHORITY\LocalService modify access to the data directory and configures Automatic start.

## IIS

Install IIS URL Rewrite and Application Request Routing, enable ARR proxy, then use deploy/iis/web.config in the HTTPS IIS site. IIS terminates HTTPS and forwards only to the local WakeBridge listener.

## Yamaha RTX flow

1. Add a Yamaha RTX Site in the UI.
2. Enter the real router host, SSH port, LAN interface, SSH username, and credential after confirming them on the router.
3. Select Test Connection. WakeBridge obtains the SSH SHA-256 fingerprint and runs the fixed read-only show version check.
4. Review the displayed fingerprint and select Trust observed fingerprint for this Site.
5. Add a Device with a normalized unicast MAC address.
6. Select Wake. The provider builds only:

~~~text
wol send -i 1 -c 3 lan1 02:00:00:00:00:10 192.0.2.10 udp 9
~~~

The interface, MAC, and optional IP are validated before command construction. Replace all example values with real values; do not commit real IPs, MACs, credentials, databases, or master keys.

Yamaha command reference: https://www.rtpro.yamaha.co.jp/RT/docs/wol/wol.html

## Example IIS topology

~~~text
Browser --HTTPS--> IIS/ARR --HTTP localhost--> WakeBridge --SSH--> Yamaha RTX
                                                               |
                                                               +--> wol send --> LAN PC
~~~

## Security notes

- SSH host-key checking cannot be bypassed for an already trusted Site.
- The first fingerprint is shown to an administrator and is not trusted automatically.
- Credential plaintext is never written to SQLite, audit entries, or tracing output.
- Keep the DPAPI master-key file and the SQLite database inside the protected data directory.
- Use a dedicated RTX account with only the permissions required by the verified firmware and command policy.

## License

WakeBridge is licensed under either the MIT License or Apache License 2.0, at your option. See LICENSE-MIT and LICENSE-APACHE.
