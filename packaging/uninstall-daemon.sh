#!/usr/bin/env bash
#
# Remove the TopFan LaunchDaemon and hand the fans back to macOS.
set -euo pipefail

LABEL="com.topfan.fand"
PLIST_DEST="/Library/LaunchDaemons/$LABEL.plist"
FAND_DEST="/usr/local/libexec/fand"
CLI_DEST="/usr/local/bin/topfan"

if [ "$(id -u)" -ne 0 ]; then
    echo "Re-running with sudo; enter your login password."
    exec sudo -- "${BASH_SOURCE[0]}" "$@"
fi

# Hand the fans back while the daemon is still alive to do it. Bootout would
# also trigger fand's own restore-on-SIGTERM, but asking explicitly means the
# fans are returned to macOS even if that path is somehow missed.
if [ -x "$CLI_DEST" ]; then
    "$CLI_DEST" off 2>/dev/null || true
fi

launchctl bootout "system/$LABEL" 2>/dev/null || true
rm -f "$PLIST_DEST" "$FAND_DEST" "$CLI_DEST"

echo "Removed the TopFan daemon. Fans are back under macOS control."
echo "The menu-bar app in /Applications is untouched; drag it to the Trash to remove it too."
