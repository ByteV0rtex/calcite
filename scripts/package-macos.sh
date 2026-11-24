#!/usr/bin/env bash
# Build Calcite.app and a .dmg to drag it into Applications.
#
#   scripts/package-macos.sh            # release build, then package into dist/
#   SKIP_BUILD=1 scripts/package-macos.sh
#
#   EDITION=ibsmp scripts/package-macos.sh   # the 9/IB SMP edition instead
#
# Signs ad hoc unless MACOS_SIGN_IDENTITY names a Developer ID certificate.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(uname -m)
TARGET=${CARGO_TARGET_DIR:-target}
DIST=dist
if [[ "${EDITION:-calcite}" == ibsmp ]]; then
  NAME="9IB SMP"
  PLIST=packaging/ibsmp/Info.plist
  ICNS=packaging/ibsmp/9IB.icns
  ICON_FILE=9IB.icns
  FEATURES=(--features calcite/ibsmp)
  DMG_BASE="9IB-SMP-$VERSION-macos-$ARCH"
else
  NAME=Calcite
  PLIST=packaging/macos/Info.plist
  ICNS=packaging/icons/Calcite.icns
  ICON_FILE=Calcite.icns
  FEATURES=()
  DMG_BASE="Calcite-$VERSION-macos-$ARCH"
fi
APP="$DIST/$NAME.app"

# CurseForge needs an API key at build time. It's never in the source:
# CI passes a secret, and locally it's read from .curseforge-key.
if [[ -z "${CALCITE_CURSEFORGE_KEY:-}" && -f .curseforge-key ]]; then
  CALCITE_CURSEFORGE_KEY=$(tr -d '[:space:]' < .curseforge-key)
  export CALCITE_CURSEFORGE_KEY
fi

if [[ -z "${SKIP_BUILD:-}" ]]; then
  # The app and its sign-in helper (Microsoft's page in a web view).
  cargo build --release -p calcite -p calcite-login ${FEATURES[@]+"${FEATURES[@]}"}
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET/release/calcite" "$APP/Contents/MacOS/calcite"
cp "$TARGET/release/calcite-login" "$APP/Contents/MacOS/calcite-login"
cp "$ICNS" "$APP/Contents/Resources/$ICON_FILE"
sed "s/@VERSION@/$VERSION/g" "$PLIST" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

ENTITLEMENTS=packaging/macos/Calcite.entitlements
if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
    --sign "$MACOS_SIGN_IDENTITY" "$APP/Contents/MacOS/calcite-login"
  codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
    --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  codesign --force --entitlements "$ENTITLEMENTS" --sign - "$APP"
fi
codesign --verify --strict "$APP"

DMG="$DIST/$DMG_BASE.dmg"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
# Right after a build macOS may still be scanning the new binary, and
# hdiutil then fails with "Resource busy": try a few times.
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname "$NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null; then
    break
  fi
  if [[ $attempt == 5 ]]; then
    echo "hdiutil couldn't make the .dmg" >&2
    exit 1
  fi
  sleep 3
done
echo "$APP"
echo "$DMG"
