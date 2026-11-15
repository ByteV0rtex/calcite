#!/usr/bin/env bash
# Fetch Modrinth's open-source launcher backend (app-lib, crate "theseus") at a
# release tag, apply Calcite's patches, and prepare it for an offline build.
#
#   scripts/sync-backend.sh v0.23.3
#
# The backend is GPL-3.0-only; Calcite links it and is GPL-3.0 as well.
set -euo pipefail

TAG="${1:-$(cat "$(dirname "$0")/../BACKEND_VERSION")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/vendor/modrinth"
REPO="https://github.com/modrinth/code.git"

echo "==> Backend tag: $TAG"

if [[ -d "$VENDOR/.git" ]]; then
  current="$(git -C "$VENDOR" describe --tags --exact-match 2>/dev/null || true)"
  if [[ "$current" != "$TAG" ]]; then
    echo "==> Replacing vendor/modrinth ($current -> $TAG)"
    rm -rf "$VENDOR"
  fi
fi

if [[ ! -d "$VENDOR/.git" ]]; then
  git clone --quiet --depth 1 --branch "$TAG" "$REPO" "$VENDOR"
fi

cd "$VENDOR"
# Start from the pristine tag, then apply Calcite's patches in order.
git reset --quiet --hard HEAD

for patch in "$ROOT"/patches/*.patch; do
  [[ -e "$patch" ]] || continue
  echo "==> Applying $(basename "$patch")"
  git apply --whitespace=nowarn "$patch"
done

# app-lib's build script reads compile-time URLs from .env.
cp packages/app-lib/.env.prod packages/app-lib/.env

echo "$TAG" > "$ROOT/BACKEND_VERSION"
echo "==> Done. Backend ready at vendor/modrinth ($TAG)"
