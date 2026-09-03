#!/usr/bin/env bash
#
# Install the TopFan root LaunchDaemon.
#
# The menu-bar app runs fine without this -- it just degrades to read-only,
# showing temperatures and RPM with the mode controls disabled. This script is
# what makes mode changes possible, because only the root daemon touches the SMC.
set -euo pipefail

LABEL="com.topfan.fand"
PLIST_DEST="/Library/LaunchDaemons/$LABEL.plist"
FAND_DEST="/usr/local/libexec/fand"
CLI_DEST="/usr/local/bin/topfan"

die() { echo "install-daemon.sh: $*" >&2; exit 1; }

# --dry-run resolves and prints the payload, then exits without touching the
# system. It is how CI smoke-tests this script without root.
DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

# Locate the payload (fand, topfan, the plist). This script ships inside
# TopFan.app/Contents/Resources, so the binaries are one directory over in
# MacOS -- but it may also be invoked from a mounted .dmg or from a source
# checkout, so try the plausible places in order.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAYLOAD=""
for candidate in \
    "$HERE/../MacOS" \
    "$HERE" \
    "$HERE/../../TopFan.app/Contents/MacOS" \
    "/Applications/TopFan.app/Contents/MacOS" \
    "$HERE/../target/release"
do
    if [ -x "$candidate/fand" ] && [ -x "$candidate/topfan" ]; then
        PAYLOAD="$(cd "$candidate" && pwd)"
        break
    fi
done
[ -n "$PAYLOAD" ] || die "could not find fand/topfan -- is TopFan.app in /Applications?"

# The plist lives beside this script inside the bundle; fall back to the
# packaging directory when running from a checkout.
PLIST_SRC=""
for candidate in "$HERE/$LABEL.plist" "$PAYLOAD/../Resources/$LABEL.plist" "$HERE/../packaging/$LABEL.plist"; do
    if [ -f "$candidate" ]; then PLIST_SRC="$candidate"; break; fi
done
[ -n "$PLIST_SRC" ] || die "could not find $LABEL.plist"

cat <<BANNER

  TopFan daemon installer

  This installs a LaunchDaemon that runs as root and controls your fans:

    $PAYLOAD/fand   ->  $FAND_DEST
    $PAYLOAD/topfan ->  $CLI_DEST
    $(basename "$PLIST_SRC")   ->  $PLIST_DEST

  The daemon can only ever raise fan speed above what macOS is already doing,
  is clamped to each fan's own reported hardware limits, and restores
  SMC-managed auto mode on startup and on exit. It does not touch thermal
  throttling or power limits.

  Uninstall later with:  sudo /Applications/TopFan.app/Contents/Resources/uninstall-daemon.sh

BANNER

if [ "$DRY_RUN" -eq 1 ]; then
    echo "  --dry-run: resolved payload and plist; nothing was changed."
    exit 0
fi

if [ -t 0 ]; then
    printf "  Continue? [y/N] "
    read -r reply
    case "$reply" in [yY]|[yY][eE][sS]) ;; *) echo "  Cancelled."; exit 0 ;; esac
fi

if [ "$(id -u)" -ne 0 ]; then
    echo
    echo "  Re-running with sudo; enter your login password."
    exec sudo -- "${BASH_SOURCE[0]}" "$@"
fi

# Stop any previous instance before replacing the binary it is running from.
# bootout exits non-zero when nothing is loaded, which is not an error here.
launchctl bootout "system/$LABEL" 2>/dev/null || true

install -d -o root -g wheel -m 755 /usr/local/libexec /usr/local/bin
install -o root -g wheel -m 755 "$PAYLOAD/fand"   "$FAND_DEST"
install -o root -g wheel -m 755 "$PAYLOAD/topfan" "$CLI_DEST"
install -o root -g wheel -m 644 "$PLIST_SRC"      "$PLIST_DEST"

# Copies inherit com.apple.quarantine from the downloaded .dmg; launchd will
# not run a quarantined daemon.
xattr -d com.apple.quarantine "$FAND_DEST" "$CLI_DEST" "$PLIST_DEST" 2>/dev/null || true

launchctl bootstrap system "$PLIST_DEST"

echo
echo "  Installed. Check it with:  topfan status"
echo "  Logs:                      log stream --predicate 'process == \"fand\"'"
echo
