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

headless_preflight() {
  local missing=()
  local found=0
  if ldconfig -p 2>/dev/null | grep -qE 'libxkbcommon-x11\.so'; then
    found=1
  elif [[ -n "${LD_LIBRARY_PATH:-}" ]]; then
    local d
    local IFS=:
    for d in $LD_LIBRARY_PATH; do
      [[ -z "$d" ]] && continue
      if compgen -G "$d/libxkbcommon-x11.so"* >/dev/null 2>&1; then
        found=1
        break
      fi
    done
  fi
  [[ "$found" -eq 1 ]] || missing+=("libxkbcommon-x11")

  found=0
  if [[ -n "${VK_ICD_FILENAMES:-}" ]]; then
    local icd
    local IFS=:
    for icd in $VK_ICD_FILENAMES; do
      [[ -z "$icd" ]] && continue
      if [[ -f "$icd" ]]; then
        found=1
        break
      fi
    done
  elif compgen -G "/usr/share/vulkan/icd.d/"*.json >/dev/null 2>&1; then
    found=1
  fi
  [[ "$found" -eq 1 ]] || missing+=("Vulkan ICD (lavapipe / mesa-vulkan-drivers)")

  if ((${#missing[@]})); then
    local joined
    joined=$(IFS=', '; echo "${missing[*]}")
    printf 'verify preflight: missing %s for GUI smoke (step 9).\n' "$joined"
    echo 'Install system packages or use the headless scratch-dir workaround in'
    echo '`.cursor/skills/verify-cofferly/SKILL.md` → "Headless libs".'
    exit 1
  fi
}

if [[ "$(uname -s)" == Linux ]]; then
  step preflight "headless libs (libxkbcommon-x11, Vulkan ICD)"
  headless_preflight
fi

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
