# AGENTS.md

Cofferly: local-only allowance wallet for kids. Rust + eframe/egui desktop app, encrypted vault, no network.

## Commands
- Format: `cargo fmt -- --check`
- Lint: `cargo clippy --all-targets --locked -- -D warnings`
- Typecheck: `cargo check --all-targets --locked`
- Unit tests: `cargo test --locked`
- Docs self-check: `scripts/check-docs.sh`
- Coverage gate: `cargo llvm-cov --locked --fail-under-lines 74 --ignore-filename-regex '(^|/)src/(views|capture)\.rs$'`
- Vault guard: `scripts/check-vault-paths.sh`
- E2E: none exist; the GUI smoke is the app's capture mode (`COFFERLY_CAPTURE`) under `xvfb-run`, wired into verify.
- **Verify entrypoint (run before every PR): `scripts/verify.sh`** Runs all of the above fail-fast, plus a docs self-check and the GUI smoke. GitHub Actions CI (`.github/workflows/ci.yml`) is live on this public repo; it must also be green on the PR.
- Drive the real app: see `.cursor/skills/verify-cofferly/SKILL.md`.

## Proof required in PRs
- Paste `scripts/verify.sh` output (tail + pass/fail).
- UI change: before/after screenshots. Logic change: test output.
- One mutation proof: revert the fix, show the new test fails, restore.

## Rules
- Never deploy or release. Never force-push or rebase a pushed branch.
- Never print secrets (story words, PINs, vault contents, tokens). Never commit `vault.cofferly`, `data.json`, `.env`.
- Always set `COFFERLY_DATA_DIR` to a temp dir when running the app; never touch a real vault.
