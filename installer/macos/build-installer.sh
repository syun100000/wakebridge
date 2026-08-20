#!/bin/bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ARCH=$(uname -m)
case "$ARCH" in arm64|x86_64) ;; *) echo "unsupported architecture: $ARCH" >&2; exit 1;; esac
VERSION=$(awk -F'"' '/^version[[:space:]]*=/ {print $2; exit}' "$ROOT/Cargo.toml")
BINARY=${1:-$ROOT/target/release/wakebridge}
[ -x "$BINARY" ] || { echo "release binary not found: $BINARY (run cargo build --release first)" >&2; exit 1; }
DIST="$ROOT/dist"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DIST" "$WORK/root/usr/local/libexec/WakeBridge" "$WORK/root/Applications/WakeBridge" "$WORK/scripts"
install -m 755 "$BINARY" "$WORK/root/usr/local/libexec/WakeBridge/wakebridge"
install -m 755 "$ROOT/installer/macos/WakeBridge-Uninstaller.command" "$WORK/root/Applications/WakeBridge/WakeBridge-Uninstaller.command"
install -m 755 "$ROOT/installer/macos/preinstall" "$WORK/scripts/preinstall"
install -m 755 "$ROOT/installer/macos/postinstall" "$WORK/scripts/postinstall"
PKG="$WORK/WakeBridge-component.pkg"
pkgbuild --root "$WORK/root" --scripts "$WORK/scripts" --identifier com.wakebridge.pkg --version "$VERSION" --install-location / "$PKG"
OUT="$DIST/WakeBridge-Setup-$VERSION-macos-$ARCH.pkg"
productbuild --package "$PKG" "$OUT"
shasum -a 256 "$OUT" | tee "$OUT.sha256"
DMG="$DIST/WakeBridge-Setup-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
hdiutil create -volname "WakeBridge $VERSION" -srcfolder "$WORK/root/Applications/WakeBridge" -format UDZO -ov "$DMG" >/dev/null
shasum -a 256 "$DMG" | tee "$DMG.sha256"
echo "Package: $OUT"
echo "DMG: $DMG"
