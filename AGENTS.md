# AGENTS.md

Cofferly: local-only allowance wallet for kids. Rust + eframe/egui desktop app, encrypted vault, no network.

## Rules
- Never deploy or release. Never force-push or rebase a pushed branch.
- Never print secrets (story words, PINs, vault contents, tokens). Never commit `vault.cofferly`, `data.json`, `.env`.
- Always set `COFFERLY_DATA_DIR` to a temp dir when running the app; never touch a real vault. Tests that touch the data dir use `ScopedDataDir` (`src/test_support.rs`).
- This repository is public. Report anything that could expose family data or weaken the vault privately (see `SECURITY.md`), never in a PR, issue, commit or comment.
- `src/story.rs` `CATALOG` ids and their order, the `coffer-story-v1:` encoding, and the envelope layout and Argon2id parameters in `src/crypto.rs` are a file format: existing vaults stop opening if any of them changes. A change needs a new format version and a migration, not an edit.
- Do not loosen a guard to get green: the vault guard, `scripts/check_security_workflows.py`, the coverage floor, `[lints.clippy]` in `Cargo.toml`, or `.cargo/audit.toml` (ignores need a written reason).

## Verify
**Run `scripts/verify.sh` before every PR.** It runs, fail-fast: vault guard, docs self-check, fmt, clippy, audit, tests, coverage gate, GUI smoke (clippy covers typecheck; run `cargo check` separately if needed). GitHub Actions CI (`.github/workflows/ci.yml`) is live on this public repo; it must also be green on the PR. CI additionally runs `python3 scripts/check_security_workflows.py`, which verify does not: run it yourself when you touch `.github/workflows/`.

Single steps:
- Format: `cargo fmt -- --check`
- Lint: `cargo clippy --all-targets --locked -- -D warnings`
- Typecheck: `cargo check --all-targets --locked`
- Unit tests: `cargo test --locked`
- Docs self-check: `scripts/check-docs.sh`
- Coverage gate: `cargo llvm-cov --locked --fail-under-lines 74 --ignore-filename-regex '(^|/)src/(views|capture)\.rs$'`
- Vault guard: `scripts/check-vault-paths.sh`
- Security audit: `cargo audit` (needs `cargo install cargo-audit --locked` and network to fetch/update the RustSec advisory DB). Accepted advisory ignores live only in `.cargo/audit.toml`. To triage a new failure: prefer updating `Cargo.lock`/dependencies; if an ignore is truly warranted, add the `RUSTSEC-…` id to `[advisories] ignore` in `.cargo/audit.toml` with a short comment in the PR.
- UI journeys: headless `egui_kittest` tests in `src/ui_tests.rs`, run by `cargo test` (`cargo test ui_tests` for only them); UI-state changes should add a journey there.
- GUI smoke: the app's capture mode (`COFFERLY_CAPTURE`) under `xvfb-run`, wired into verify.
- Drive the real app: see `.cursor/skills/verify-cofferly/SKILL.md`.

## Purpose and proof (required)

Every agent-filed issue and every PR must start with the Purpose block from the `.github` templates. Apply labels `priority:P1` | `priority:P2` | `priority:P3` and `value:high` | `value:medium` | `value:low`. A missing or vague Purpose block is a Gabriel HOLD. Dependabot and one-line chores are exempt.

PR proof: paste `scripts/verify.sh` output (tail + pass/fail); for a UI change, before/after screenshots; for a logic change, test output; and one mutation proof (revert the fix, show the new test fails, restore).

## Gotchas
- `cargo audit` prints warnings for unmaintained or yanked transitive crates and still passes; only vulnerabilities fail it (`SECURITY.md`).
- The pre-push hook is opt-in: `git config core.hooksPath .githooks`. A fresh clone pushes without it.
- `scripts/check-docs.sh` fails verify if this file, `CLAUDE.md`, `scripts/verify.sh` or the verify skill describe CI as unavailable, or if the skill's screenshot names drift from `src/capture.rs`.
- Money is integer cents (`i64`) with checked or saturating arithmetic and a `MAX_ABSOLUTE_CENTS` clamp in `src/data.rs`. Never introduce floats for amounts.
- `clippy::unwrap_used` is a warning and warnings are errors: return a `Result` outside tests.
