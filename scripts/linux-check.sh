#!/usr/bin/env bash
# Build and test Calcite for Linux inside Docker. Build output stays in Docker
# volumes, so the host's target/ is untouched.
#
#   scripts/linux-check.sh            # build + test
#   scripts/linux-check.sh bash       # drop into a shell in the container
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE=calcite-linux-build

docker build -q -t "$IMAGE" "$ROOT/scripts/linux" >/dev/null

args=(--rm -v "$ROOT:/src" -v calcite-linux-target:/target -v calcite-linux-cargo:/usr/local/cargo/registry -v calcite-linux-gradle:/gradle)
if [[ "${1:-}" == "bash" ]]; then
  exec docker run -it "${args[@]}" "$IMAGE" bash
fi

docker run "${args[@]}" "$IMAGE" bash -c '
  set -e
  mkdir -p "$HOME"
  cargo test --workspace 2>&1 | grep -E "^(test result|error|warning: unused)" || true
  cargo build -p calcite 2>&1 | tail -3
  ls -la /target/debug/calcite
'
