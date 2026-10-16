#!/usr/bin/env bash
# Single local verify entrypoint (CI is local-only). Fails fast, in order:
# vault guard, fmt, clippy (lint), build/typecheck, unit tests, coverage gate,
# GUI smoke (the app's own capture mode under a virtual display).
# Usage: scripts/verify.sh
# Step 7 needs xkbcommon-x11 + a Vulkan ICD (lavapipe). If missing, see the verify-cofferly skill "Headless libs".
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n=== [%s] %s ===\n' "$1" "$2"; }

step 1/7 "vault guard"
./scripts/check-vault-paths.sh

step 2/7 "format"
cargo fmt -- --check

step 3/7 "lint (clippy)"
cargo clippy --all-targets --locked -- -D warnings

step 4/7 "typecheck (cargo check)"
cargo check --all-targets --locked

step 5/7 "unit tests"
cargo test --locked

step 6/7 "coverage gate (>=74% lines, views/capture excluded)"
cargo llvm-cov --locked --fail-under-lines 74 \
  --ignore-filename-regex '(^|/)src/(views|capture)\.rs$'

step 7/7 "GUI smoke (capture story-unlock, isolated data dir)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cargo build --release --locked
run=("target/release/Cofferly")
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  command -v xvfb-run >/dev/null || { echo "no display and no xvfb-run"; exit 1; }
  run=(xvfb-run -a "${run[@]}")
fi
mkdir -p "$tmp/data" "$tmp/out"
COFFERLY_DATA_DIR="$tmp/data" COFFERLY_CAPTURE=story-unlock COFFERLY_CAPTURE_DIR="$tmp/out" \
  timeout 180 "${run[@]}"
test -s "$tmp/out/cofferly-story-unlock.png" || { echo "screenshot missing"; exit 1; }
echo "smoke OK: $(stat -c %s "$tmp/out/cofferly-story-unlock.png") byte screenshot"

echo; echo "VERIFY PASSED"
