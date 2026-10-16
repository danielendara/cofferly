#!/usr/bin/env bash
# Docs self-check for agent instructions:
# 1. fail if AGENTS.md / the verify skill claim CI is unavailable while ci.yml exists;
# 2. fail if the skill names a capture PNG that CaptureTarget::file_name does not write,
#    or omits one it does write.
set -euo pipefail
cd "$(dirname "$0")/.."

docs=(AGENTS.md CLAUDE.md scripts/verify.sh .cursor/skills/verify-cofferly)
fail=0

if [[ -f .github/workflows/ci.yml ]]; then
  if grep -rniE "ci is (local|down|unavailable)|local-only[ ,;(]*(ci|verify)|actions (is|are) down|no ci\b" "${docs[@]}" --exclude=check-docs.sh; then
    echo "::error::docs claim CI is unavailable but .github/workflows/ci.yml exists"
    fail=1
  fi
fi

actual="$(grep -oE '"cofferly-[a-z-]+\.png"' src/capture.rs | tr -d '"' | sort -u)"
[[ -n "$actual" ]] || { echo "::error::no capture filenames found in src/capture.rs"; exit 1; }
mentioned="$(grep -rhoE 'cofferly-[a-z-]+\.png' .cursor/skills/verify-cofferly | sort -u)"
skill=.cursor/skills/verify-cofferly/SKILL.md

for f in $mentioned; do
  grep -qx "$f" <<<"$actual" || { echo "::error::skill mentions $f, which capture.rs never writes"; fail=1; }
done
for f in $actual; do
  grep -q "$f" "$skill" || { echo "::error::SKILL.md does not list capture file $f"; fail=1; }
done

[[ $fail -eq 0 ]] && echo "docs check OK"
exit $fail
