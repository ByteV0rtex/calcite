#!/usr/bin/env bash
# Build Calcite.app and a .dmg to drag it into Applications.
#
#   scripts/package-macos.sh            # release build, then package into dist/
#   SKIP_BUILD=1 scripts/package-macos.sh
#
# Signs ad hoc unless MACOS_SIGN_IDENTITY names a Developer ID certificate.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(uname -m)
TARGET=${CARGO_TARGET_DIR:-target}
DIST=dist
APP="$DIST/Calcite.app"

if [[ -z "${SKIP_BUILD:-}" ]]; then
  cargo build --release -p calcite
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET/release/calcite" "$APP/Contents/MacOS/calcite"
cp packaging/icons/Calcite.icns "$APP/Contents/Resources/Calcite.icns"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"

DMG="$DIST/Calcite-$VERSION-macos-$ARCH.dmg"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname Calcite -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "$APP"
echo "$DMG"
