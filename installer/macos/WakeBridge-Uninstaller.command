#!/bin/bash
set -euo pipefail
BIN=/usr/local/libexec/WakeBridge/wakebridge
DATA="/Library/Application Support/WakeBridge"
PLIST=/Library/LaunchDaemons/com.wakebridge.service.plist
SELF="$0"
if [ "${1:-}" = "--delete-data" ]; then
  delete_data=1
elif [ "${1:-}" = "" ]; then
  answer=$(/usr/bin/osascript -e 'display dialog "WakeBridgeのデータも削除しますか？\n\nSQLite、ユーザー、暗号化Credential、master keyを含む不可逆操作です。" buttons {"保持", "削除"} default button "保持" with title "WakeBridgeアンインストーラー" with icon caution' 2>/dev/null || true)
  case "$answer" in *削除*) delete_data=1;; *) delete_data=0;; esac
else
  echo "Usage: $0 [--delete-data]" >&2
  exit 2
fi
if [ "$(/usr/bin/id -u)" -ne 0 ]; then
  if [ "$delete_data" = 1 ]; then exec /usr/bin/sudo "$SELF" --delete-data; else exec /usr/bin/sudo "$SELF"; fi
fi
if [ -x "$BIN" ]; then
  "$BIN" service uninstall
elif [ -f "$PLIST" ]; then
  /bin/launchctl bootout system "$PLIST"
fi
rm -f "$BIN" "$PLIST"
if [ "$delete_data" = 1 ]; then rm -rf "$DATA"; fi
rm -f "$SELF"
echo "WakeBridgeをアンインストールしました。データ: $([ "$delete_data" = 1 ] && echo 削除 || echo 保持)"
