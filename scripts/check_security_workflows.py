#!/usr/bin/env python3
"""Ensure workflow files pin third-party actions to immutable commit SHAs."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

USES_RE = re.compile(r"^\s*(?:-\s+)?uses:\s*(.+?)\s*(?:#.*)?$")
SHA_AT = re.compile(r"^[^@]+@([0-9a-f]{40})$")
ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = (
    ROOT / ".github/workflows/ci.yml",
    ROOT / ".github/workflows/release.yml",
)


def check_file(path: Path) -> list[str]:
    errors: list[str] = []
    for i, line in enumerate(path.read_text().splitlines(), 1):
        m = USES_RE.match(line)
        if not m:
            continue
        ref = m.group(1).strip()
        if ref.startswith("./") or ref.startswith("docker://"):
            continue
        if "@" not in ref:
            errors.append(f"{path}:{i}: missing @ref in uses: {ref}")
            continue
        if not SHA_AT.match(ref):
            errors.append(f"{path}:{i}: unpinned action ref: {ref}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fixtures",
        action="store_true",
        help="run good/bad fixture checks under scripts/fixtures/",
    )
    args = parser.parse_args()
    paths = list(WORKFLOWS)
    if args.fixtures:
        fix = ROOT / "scripts/fixtures"
        good = fix / "workflow-pinned.yml"
        bad = fix / "workflow-unpinned.yml"
        if check_file(good):
            print("fixture workflow-pinned.yml should pass", file=sys.stderr)
            return 1
        bad_errs = check_file(bad)
        if not bad_errs:
            print("fixture workflow-unpinned.yml should fail", file=sys.stderr)
            return 1
        paths = [good]

    errors: list[str] = []
    for p in paths:
        if not p.is_file():
            errors.append(f"missing workflow: {p}")
            continue
        errors.extend(check_file(p))
    if errors:
        for e in errors:
            print(e, file=sys.stderr)
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
