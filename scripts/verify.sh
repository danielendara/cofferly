#!/usr/bin/env bash
# Single local verify entrypoint (mirrors and extends GitHub Actions CI). Fails fast, in order:
# vault guard, docs self-check, fmt, clippy (lint), build/typecheck, cargo audit, unit tests,
# coverage gate, GUI smoke (the app's own capture mode under a virtual display).
# Usage: scripts/verify.sh
# Audit step needs network (RustSec advisory DB). Install: cargo install cargo-audit --locked
# Step 9 needs xkbcommon-x11 + a Vulkan ICD (lavapipe). If missing, see the verify-cofferly skill "Headless libs".
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n=== [%s] %s ===\n' "$1" "$2"; }

step 1/9 "vault guard"
./scripts/check-vault-paths.sh

step 2/9 "docs self-check"
./scripts/check-docs.sh

step 3/9 "format"
cargo fmt -- --check

step 4/9 "lint (clippy)"
cargo clippy --all-targets --locked -- -D warnings

step 5/9 "typecheck (cargo check)"
cargo check --all-targets --locked

step 6/9 "security audit (cargo audit)"
command -v cargo-audit >/dev/null || {
  echo "cargo-audit not found; install with: cargo install cargo-audit --locked"
  exit 1
}
cargo audit

step 7/9 "unit tests"
cargo test --locked

step 8/9 "coverage gate (>=74% lines, views/capture excluded)"
cargo llvm-cov --locked --fail-under-lines 74 \
  --ignore-filename-regex '(^|/)src/(views|capture)\.rs$'

step 9/9 "GUI smoke (capture story-unlock, isolated data dir)"
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
