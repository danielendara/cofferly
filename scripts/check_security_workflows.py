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
STEP_START_RE = re.compile(r"^      - (?:name:|uses:)")
WITH_KEY_RE = re.compile(r"^          ([\w_]+):\s*(.*)$")
WITH_ITEM_RE = re.compile(r"^            (.+)$")
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


def _find_uses_with(job_lines: list[str], action_needle: str) -> dict[str, str | list[str]]:
    """Return the with: mapping for the first step whose uses: contains action_needle."""
    for i, line in enumerate(job_lines):
        if "uses:" not in line or action_needle not in line:
            continue
        for j in range(i + 1, len(job_lines)):
            if job_lines[j].startswith("      - "):
                break
            if job_lines[j].strip() == "with:":
                return _read_with_block(job_lines, j + 1)
    return {}


def _read_with_block(job_lines: list[str], start: int) -> dict[str, str | list[str]]:
    values: dict[str, str | list[str]] = {}
    i = start
    while i < len(job_lines) and not STEP_START_RE.match(job_lines[i]):
        m = WITH_KEY_RE.match(job_lines[i])
        if not m:
            if job_lines[i].strip():
                break
            i += 1
            continue
        key, rest = m.group(1), m.group(2).strip()
        if rest == "|":
            items: list[str] = []
            i += 1
            while i < len(job_lines):
                if STEP_START_RE.match(job_lines[i]):
                    break
                if WITH_KEY_RE.match(job_lines[i]):
                    break
                im = WITH_ITEM_RE.match(job_lines[i])
                if im:
                    items.append(im.group(1).strip())
                    i += 1
                    continue
                if not job_lines[i].strip():
                    i += 1
                    continue
                break
            values[key] = items
            continue
        values[key] = rest
        i += 1
    return values


def _path_entries(with_map: dict[str, str | list[str]], key: str) -> list[str]:
    raw = with_map.get(key)
    if raw is None:
        return []
    if isinstance(raw, list):
        return [p for p in raw if p.strip()]
    return [raw] if raw.strip() else []


def _upload_artifact_root(upload_paths: list[str]) -> str | None:
    dirs: list[str] = []
    for entry in upload_paths:
        entry = entry.strip().replace("\\", "/")
        if "/" in entry:
            dirs.append(entry.rsplit("/", 1)[0])
        else:
            dirs.append(".")
    if not dirs:
        return None
    unique = set(dirs)
    if len(unique) != 1:
        return None
    return dirs[0]


def _glob_under_prefix(glob_pattern: str, prefix: str) -> bool:
    norm = glob_pattern.strip().replace("\\", "/")
    if prefix == ".":
        return "/" not in norm or norm.startswith("./")
    return norm == prefix or norm.startswith(f"{prefix}/")


def _truthy(val: str | list[str] | None) -> bool:
    if val is None:
        return False
    if isinstance(val, list):
        return False
    return val.strip().lower() in ("true", "yes", "1")


def check_release_artifact_handoff(path: Path) -> list[str]:
    """Ensure upload, download, and release file globs share the same directory layout."""
    errors: list[str] = []
    jobs = _split_jobs(path.read_text().splitlines())
    build_body = jobs.get(BUILD_JOB, [])
    publish_body = jobs.get(PUBLISH_JOB, [])
    if not build_body or not publish_body:
        return errors

    upload_with = _find_uses_with(build_body, "actions/upload-artifact")
    download_with = _find_uses_with(publish_body, "actions/download-artifact")
    release_with = _find_uses_with(publish_body, "softprops/action-gh-release")

    upload_paths = _path_entries(upload_with, "path")
    if not upload_paths:
        errors.append(f"{path}: {BUILD_JOB!r} upload-artifact must set path")
        return errors

    upload_root = _upload_artifact_root(upload_paths)
    if upload_root is None:
        errors.append(
            f"{path}: upload-artifact paths must share one directory prefix, got {upload_paths!r}"
        )
        return errors

    download_entries = _path_entries(download_with, "path")
    download_path = download_entries[0] if download_entries else ""
    download_path = download_path.strip().replace("\\", "/")
    if download_path != upload_root:
        errors.append(
            f"{path}: download-artifact path {download_path!r} must equal upload root {upload_root!r}"
        )

    release_globs = _path_entries(release_with, "files")
    if not release_globs:
        errors.append(f"{path}: publish release step must set files globs")
    for pattern in release_globs:
        if not _glob_under_prefix(pattern, upload_root):
            errors.append(
                f"{path}: release glob {pattern!r} must be under upload/download root {upload_root!r}"
            )

    if not _truthy(release_with.get("fail_on_unmatched_files")):
        errors.append(f"{path}: release step must set fail_on_unmatched_files: true")

    return errors


def check_release_workflow(path: Path) -> list[str]:
    errors = check_release_least_privilege(path)
    errors.extend(check_release_artifact_handoff(path))
    return errors


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
    layout_bad = fix / "release-artifact-layout-bad.yml"
    if check_release_workflow(good):
        print("fixture release-least-privilege-good.yml should pass", file=sys.stderr)
        return 1
    if not check_release_least_privilege(bad):
        print("fixture release-least-privilege-bad.yml should fail", file=sys.stderr)
        return 1
    if not check_release_artifact_handoff(layout_bad):
        print("fixture release-artifact-layout-bad.yml should fail", file=sys.stderr)
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
            errors.extend(check_release_workflow(p))
    if errors:
        for e in errors:
            print(e, file=sys.stderr)
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())