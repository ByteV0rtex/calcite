#!/usr/bin/env bash
# Regenerate packaging/icons from the app icon SVG (run after changing it).
# Needs macOS for iconutil (the .icns); the rest works anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=packaging/icons
mkdir -p "$OUT"
cargo run -q -p calcite-core --example brand_icons -- crates/calcite/assets/brand/app-icon.svg "$OUT"
iconutil -c icns "$OUT/Calcite.iconset" -o "$OUT/Calcite.icns"
rm -rf "$OUT/Calcite.iconset"
ls -la "$OUT"
