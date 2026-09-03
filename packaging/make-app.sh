#!/usr/bin/env bash
#
# Assemble TopFan.app from binaries cargo has already built.
#
# The bundle carries all three binaries: the menu-bar GUI as the bundle
# executable, plus `topfan` and `fand`, which the daemon installer copies into
# place. Keeping them inside the app means the daemon can be installed (or
# reinstalled) after the .dmg has been ejected.
#
# Auxiliary executables live in Contents/MacOS, not Contents/Resources:
# codesign treats Mach-O files under Resources as a bundle-layout error, which
# fails notarization.
set -euo pipefail

VERSION="0.0.0-dev"
BIN_DIR=""
OUT_DIR=""
SIGN_ID="-"   # ad-hoc by default: arm64 refuses to run wholly unsigned code

usage() {
    cat <<'USAGE'
usage: make-app.sh [--version X.Y.Z] [--bin-dir DIR] [--out DIR] [--sign IDENTITY]

  --version    version string baked into Info.plist   (default 0.0.0-dev)
  --bin-dir    where TopFan/topfan/fand were built    (default target/release)
  --out        directory to create TopFan.app in      (default dist)
  --sign       codesign identity, "-" for ad-hoc      (default -)
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --bin-dir) BIN_DIR="$2"; shift 2 ;;
        --out)     OUT_DIR="$2"; shift 2 ;;
        --sign)    SIGN_ID="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "make-app.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BIN_DIR="${BIN_DIR:-$ROOT/target/release}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
APP="$OUT_DIR/TopFan.app"

for bin in menubar topfan fand; do
    if [ ! -x "$BIN_DIR/$bin" ]; then
        echo "make-app.sh: missing $BIN_DIR/$bin -- run 'cargo build --release' first" >&2
        exit 1
    fi
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# The bundle executable must match CFBundleExecutable. It is deliberately NOT
# called "TopFan": the stock macOS filesystem is case-insensitive, so a bundle
# executable named TopFan and the bundled CLI named topfan are the SAME FILE.
# Installing both left the app launching the CLI, which prints usage and exits
# -- a bundle that builds, signs, verifies, and shows no menu-bar item.
install -m 755 "$BIN_DIR/menubar" "$APP/Contents/MacOS/TopFanMenuBar"
install -m 755 "$BIN_DIR/topfan"  "$APP/Contents/MacOS/topfan"
install -m 755 "$BIN_DIR/fand"    "$APP/Contents/MacOS/fand"

# Guard the above: three distinct executables must survive the copy, and the
# bundle executable must still be byte-identical to the menubar binary.
count="$(find "$APP/Contents/MacOS" -type f | wc -l | tr -d ' ')"
[ "$count" -eq 3 ] || {
    echo "make-app.sh: expected 3 executables in Contents/MacOS, found $count" >&2
    echo "make-app.sh: names probably collided on a case-insensitive filesystem" >&2
    exit 1
}
[ "$(shasum -a 256 < "$BIN_DIR/menubar")" = "$(shasum -a 256 < "$APP/Contents/MacOS/TopFanMenuBar")" ] || {
    echo "make-app.sh: bundle executable is not the menubar binary" >&2
    exit 1
}

install -m 644 "$HERE/com.topfan.fand.plist" "$APP/Contents/Resources/"
install -m 755 "$HERE/install-daemon.sh"     "$APP/Contents/Resources/"
install -m 755 "$HERE/uninstall-daemon.sh"   "$APP/Contents/Resources/"

sed "s/__VERSION__/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

plutil -lint "$APP/Contents/Info.plist" >/dev/null

# Sign inner executables before the bundle: the outer signature seals their
# hashes, so signing them afterwards would invalidate it. --deep is deprecated
# and does the wrong thing for nested Mach-O, so each is named explicitly.
SIGN_ARGS=(--force --timestamp --options runtime)
if [ "$SIGN_ID" = "-" ]; then
    # Ad-hoc signatures cannot carry a secure timestamp.
    SIGN_ARGS=(--force)
fi
for bin in topfan fand TopFanMenuBar; do
    codesign "${SIGN_ARGS[@]}" --sign "$SIGN_ID" "$APP/Contents/MacOS/$bin"
done
codesign "${SIGN_ARGS[@]}" --sign "$SIGN_ID" "$APP"

codesign --verify --strict --verbose=2 "$APP"

echo "make-app.sh: built $APP (version $VERSION, signed by '$SIGN_ID')"
