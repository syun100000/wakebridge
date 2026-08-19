# WakeBridge

WakeBridge is a Windows-native Rust service for managing Wake-on-LAN across multiple sites. It sends a fixed Yamaha RTX wol send command over SSH; WakeBridge never broadcasts UDP directly from the Windows host and exposes no arbitrary SSH command API.

## Features

- Rust stable MSVC binary, Tokio, Axum, SQLite, Askama templates, and vanilla JavaScript
- Sites, devices, users, settings, wake history, and audit history
- Yamaha RTX provider behind a provider trait
- SSH host-key fingerprint trust on first connection and strict mismatch rejection afterwards
- SSH credentials encrypted with AES-256-GCM under a machine-protected, installation-specific DPAPI master key
- Argon2id passwords, in-memory sessions, HttpOnly/SameSite cookies, CSRF tokens, and login throttling
- Windows Service commands with LocalService and automatic start
- IIS reverse-proxy configuration for 127.0.0.1:8787

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

The native executable is target\release\wakebridge.exe.

## First run

The default runtime is:

- listen: 127.0.0.1:8787
- data: C:\ProgramData\WakeBridge
- binary: C:\Program Files\WakeBridge\wakebridge.exe

Create the first administrator. If --password is omitted, a random password is printed once:

~~~powershell
.\wakebridge.exe user create --username admin --role admin
.\wakebridge.exe run
~~~

For local HTTP testing only, set WAKEBRIDGE_DEV_INSECURE_COOKIE=1. Keep the database setting Secure Cookie enabled when IIS terminates HTTPS.

## Windows Service

Run an elevated PowerShell after building:

~~~powershell
.\wakebridge.exe service install
.\wakebridge.exe user create --username admin --role admin
.\wakebridge.exe service start
.\wakebridge.exe service status
.\wakebridge.exe service stop
.\wakebridge.exe service uninstall
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
