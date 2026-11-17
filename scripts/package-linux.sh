#!/usr/bin/env bash
# Build a .deb and an AppImage into dist/.
#
#   scripts/package-linux.sh             # release build, then package
#   SKIP_BUILD=1 scripts/package-linux.sh
#
# The AppImage needs appimagetool on PATH (or APPIMAGETOOL=/path/to/it);
# without it only the .deb is made.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TARGET=${CARGO_TARGET_DIR:-target}
DIST=dist
ID=com.calcite.launcher
case "$(uname -m)" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64) DEB_ARCH=arm64 ;;
  *) DEB_ARCH=$(uname -m) ;;
esac

# CurseForge needs an API key at build time. It's never in the source:
# CI passes a secret, and locally it's read from .curseforge-key.
if [[ -z "${CALCITE_CURSEFORGE_KEY:-}" && -f .curseforge-key ]]; then
  CALCITE_CURSEFORGE_KEY=$(tr -d '[:space:]' < .curseforge-key)
  export CALCITE_CURSEFORGE_KEY
fi

if [[ -z "${SKIP_BUILD:-}" ]]; then
  # The app and its sign-in helper (Microsoft's page in a web view).
  cargo build --release -p calcite -p calcite-login
fi
mkdir -p "$DIST"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# One file tree, used by both packages.
install_tree() {
  local root=$1
  install -Dm755 "${CALCITE_BIN:-$TARGET/release/calcite}" "$root/usr/bin/calcite"
  # Calcite looks for the sign-in helper next to itself.
  install -Dm755 "${CALCITE_LOGIN_BIN:-$TARGET/release/calcite-login}" "$root/usr/bin/calcite-login"
  install -Dm644 "packaging/linux/$ID.desktop" "$root/usr/share/applications/$ID.desktop"
  install -Dm644 packaging/linux/calcite-mrpack.xml "$root/usr/share/mime/packages/$ID.xml"
  install -Dm644 packaging/icons/calcite-256.png "$root/usr/share/icons/hicolor/256x256/apps/$ID.png"
  install -Dm644 packaging/icons/calcite-512.png "$root/usr/share/icons/hicolor/512x512/apps/$ID.png"
  install -Dm644 LICENSE "$root/usr/share/doc/calcite/copyright"
}

# .deb
DEB_ROOT="$WORK/deb"
install_tree "$DEB_ROOT"
mkdir -p "$DEB_ROOT/DEBIAN"
cat > "$DEB_ROOT/DEBIAN/control" <<CONTROL
Package: calcite
Version: $VERSION
Section: games
Priority: optional
Architecture: $DEB_ARCH
Depends: libc6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1, libx11-xcb1, libxcb1, libfontconfig1, libfreetype6, libgtk-3-0, libwebkit2gtk-4.1-0
Recommends: mesa-vulkan-drivers
Installed-Size: $(du -sk "$DEB_ROOT/usr" | cut -f1)
Maintainer: ByteV0rtex <https://github.com/ByteV0rtex/calcite>
Homepage: https://github.com/ByteV0rtex/calcite
Description: A fast, native Minecraft launcher
 Calcite installs and plays Minecraft modpacks from Modrinth, with no ads
 and no tracking. Instances share their files, so they take little space.
CONTROL
cat > "$DEB_ROOT/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
command -v update-mime-database >/dev/null && update-mime-database /usr/share/mime || true
command -v update-desktop-database >/dev/null && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q /usr/share/icons/hicolor || true
POST
chmod 755 "$DEB_ROOT/DEBIAN/postinst"
cp "$DEB_ROOT/DEBIAN/postinst" "$DEB_ROOT/DEBIAN/postrm"
DEB="$DIST/calcite_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DEB" >/dev/null
echo "$DEB"

# AppImage
TOOL=${APPIMAGETOOL:-$(command -v appimagetool || true)}
if [[ -z "$TOOL" ]]; then
  echo "appimagetool not found: skipped the AppImage" >&2
  exit 0
fi
APPDIR="$WORK/Calcite.AppDir"
install_tree "$APPDIR"
cp "packaging/linux/$ID.desktop" "$APPDIR/$ID.desktop"
cp packaging/icons/calcite-256.png "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/calcite" "$@"
RUN
chmod 755 "$APPDIR/AppRun"
IMAGE="$DIST/Calcite-$VERSION-linux-$(uname -m).AppImage"
ARCH=$(uname -m) APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$IMAGE" >/dev/null
echo "$IMAGE"
