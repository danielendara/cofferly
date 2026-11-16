#!/usr/bin/env python3
"""Ensure workflow files pin third-party actions and release.yml uses least privilege."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

USES_RE = re.compile(r"^\s*(?:-\s+)?uses:\s*(.+?)\s*(?:#.*)?$")
SHA_AT = re.compile(r"^[^@]+@([0-9a-f]{40})$")
CONTENTS_RE = re.compile(r"^\s*contents:\s*(\S+)\s*(?:#.*)?$")
NEEDS_RE = re.compile(r"^\s*needs:\s*(.+)\s*$")
JOB_START_RE = re.compile(r"^  ([A-Za-z][\w-]*):\s*$")
ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = (
    ROOT / ".github/workflows/ci.yml",
    ROOT / ".github/workflows/release.yml",
)
BUILD_JOB = "windows"
PUBLISH_JOB = "publish"
FORBIDDEN_PUBLISH_USES = (
    "dtolnay/rust-toolchain",
    "Swatinem/rust-cache",
    "actions/cache@",
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


def _top_level_block(lines: list[str], key: str) -> list[str] | None:
    start: int | None = None
    for i, line in enumerate(lines):
        if line == f"{key}:":
            start = i + 1
            break
    if start is None:
        return None
    block: list[str] = []
    for line in lines[start:]:
        if line and not line.startswith(" "):
            break
        block.append(line)
    return block


def _contents_permission(block: list[str] | None) -> str | None:
    if not block:
        return None
    for line in block:
        m = CONTENTS_RE.match(line)
        if m:
            return m.group(1)
    return None


def _split_jobs(lines: list[str]) -> dict[str, list[str]]:
    jobs_start: int | None = None
    for i, line in enumerate(lines):
        if line == "jobs:":
            jobs_start = i + 1
            break
    if jobs_start is None:
        return {}
    jobs: dict[str, list[str]] = {}
    current: str | None = None
    for line in lines[jobs_start:]:
        if line and not line.startswith(" "):
            break
        m = JOB_START_RE.match(line)
        if m:
            current = m.group(1)
            jobs[current] = []
            continue
        if current is not None:
            jobs[current].append(line)
    return jobs


def check_release_least_privilege(path: Path) -> list[str]:
    """Assert release workflow keeps contents:write only on the publish job."""
    errors: list[str] = []
    text = path.read_text()
    lines = text.splitlines()

    wf_perm = _contents_permission(_top_level_block(lines, "permissions"))
    if wf_perm != "read":
        errors.append(
            f"{path}: workflow permissions.contents must be read (got {wf_perm!r})"
        )

    jobs = _split_jobs(lines)
    if BUILD_JOB not in jobs:
        errors.append(f"{path}: missing build job {BUILD_JOB!r}")
    if PUBLISH_JOB not in jobs:
        errors.append(f"{path}: missing publish job {PUBLISH_JOB!r}")
    if not jobs:
        return errors

    build_body = jobs.get(BUILD_JOB, [])
    build_perm = _contents_permission(build_body)
    if build_perm == "write":
        errors.append(f"{path}: job {BUILD_JOB!r} must not set contents: write")

    publish_body = jobs.get(PUBLISH_JOB, [])
    publish_perm = _contents_permission(publish_body)
    if publish_perm != "write":
        errors.append(
            f"{path}: job {PUBLISH_JOB!r} must set permissions.contents: write"
        )

    needs_ok = False
    for line in publish_body:
        m = NEEDS_RE.match(line)
        if not m:
            continue
        needs_val = m.group(1).strip()
        needs_parts = [p.strip() for p in needs_val.split(",")]
        if BUILD_JOB in needs_parts:
            needs_ok = True
            break
    if not needs_ok:
        errors.append(f"{path}: job {PUBLISH_JOB!r} must need {BUILD_JOB!r}")

    for job_name, job_lines in jobs.items():
        perm = _contents_permission(job_lines)
        if perm == "write" and job_name != PUBLISH_JOB:
            errors.append(f"{path}: only {PUBLISH_JOB!r} may set contents: write")

    if PUBLISH_JOB in jobs:
        for i, line in enumerate(publish_body, 1):
            m = USES_RE.match(line)
            if not m:
                continue
            ref = m.group(1).strip()
            for forbidden in FORBIDDEN_PUBLISH_USES:
                if forbidden in ref:
                    errors.append(
                        f"{path}: publish job must not use toolchain/cache action: {ref}"
                    )

    return errors


def run_release_fixture_checks(fix: Path) -> int:
    good = fix / "release-least-privilege-good.yml"
    bad = fix / "release-least-privilege-bad.yml"
    if check_release_least_privilege(good):
        print("fixture release-least-privilege-good.yml should pass", file=sys.stderr)
        return 1
    if not check_release_least_privilege(bad):
        print("fixture release-least-privilege-bad.yml should fail", file=sys.stderr)
        return 1
    return 0


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
        if run_release_fixture_checks(fix):
            return 1
        paths = [good]

    errors: list[str] = []
    for p in paths:
        if not p.is_file():
            errors.append(f"missing workflow: {p}")
            continue
        errors.extend(check_file(p))
        if p.name == "release.yml":
            errors.extend(check_release_least_privilege(p))
    if errors:
        for e in errors:
            print(e, file=sys.stderr)
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())