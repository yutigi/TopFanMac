#!/usr/bin/env bash
#
# Build the distributable .dmg around an already-assembled TopFan.app.
#
# Layout is the familiar drag-install: the app, a symlink to /Applications, and
# the daemon installer alongside, because dragging an app into /Applications
# cannot install a root LaunchDaemon.
set -euo pipefail

VERSION="0.0.0-dev"
APP=""
OUT=""

usage() {
    cat <<'USAGE'
usage: make-dmg.sh [--version X.Y.Z] [--app PATH/TopFan.app] [--out FILE.dmg]

  --version    version string, used in the .dmg filename and volume name
  --app        the bundle to package         (default dist/TopFan.app)
  --out        output path                   (default dist/TopFan-<version>-arm64.dmg)
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --app)     APP="$2"; shift 2 ;;
        --out)     OUT="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "make-dmg.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
APP="${APP:-$ROOT/dist/TopFan.app}"
OUT="${OUT:-$ROOT/dist/TopFan-$VERSION-arm64.dmg}"

[ -d "$APP" ] || { echo "make-dmg.sh: no bundle at $APP -- run make-app.sh first" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp -R "$APP" "$STAGE/TopFan.app"
ln -s /Applications "$STAGE/Applications"

# Convenience launcher. Gatekeeper blocks double-clicking an unsigned .command
# off a quarantined disk image, so the README points at the Terminal one-liner
# as the primary path and treats this as the shortcut when it does work.
cat > "$STAGE/Install Fan Daemon.command" <<'CMD'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for app in "$HERE/TopFan.app" "/Applications/TopFan.app"; do
    if [ -x "$app/Contents/Resources/install-daemon.sh" ]; then
        exec "$app/Contents/Resources/install-daemon.sh"
    fi
done
echo "Could not find TopFan.app. Copy it to /Applications first, then run:"
echo "  sudo /Applications/TopFan.app/Contents/Resources/install-daemon.sh"
read -r -p "Press return to close." _
CMD
chmod +x "$STAGE/Install Fan Daemon.command"

cat > "$STAGE/README.txt" <<'TXT'
TopFan
======

1. Drag TopFan.app onto the Applications folder.

2. Install the fan-control daemon. Open Terminal and run:

       sudo /Applications/TopFan.app/Contents/Resources/install-daemon.sh

   (The "Install Fan Daemon" shortcut on this disk image does the same thing,
   but macOS may refuse to open it directly from a downloaded image.)

Step 2 is what enables fan control. Without it the app still runs and shows
live temperatures and fan RPM, but the Auto/Managed/Full/Off controls stay
disabled, because only the root daemon is allowed to touch the SMC.

If macOS says the app "cannot be opened because the developer cannot be
verified", right-click TopFan.app and choose Open, or clear the quarantine
flag:

    xattr -dr com.apple.quarantine /Applications/TopFan.app

To uninstall:

    sudo /Applications/TopFan.app/Contents/Resources/uninstall-daemon.sh

then drag TopFan.app to the Trash. Uninstalling hands the fans back to macOS.
TXT

rm -f "$OUT"
mkdir -p "$(dirname "$OUT")"
hdiutil create \
    -volname "TopFan $VERSION" \
    -srcfolder "$STAGE" \
    -format UDZO \
    -ov \
    "$OUT" >/dev/null

echo "make-dmg.sh: built $OUT"
