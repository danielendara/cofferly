# Contributing

Cofferly is a maintainer-led family app. The repository is public so people can learn from it, audit it, fork it, and propose useful improvements, but it is not open for direct unreviewed changes.

## Contribution Policy

- Open an issue before starting larger work.
- Keep pull requests focused and small.
- Do not include real children's names, family data, screenshots with private data, or personal ledger entries.
- Do not add telemetry, cloud sync, ads, accounts, or network features without prior maintainer approval.
- Do not change licensing, release automation, or security-sensitive behavior without prior discussion.
- All pull requests require maintainer review before merging.

## Local Development

Run the full local gate before opening a pull request:

```bash
scripts/verify.sh
```

That script runs `scripts/check-vault-paths.sh`, `scripts/check-docs.sh`, formatting, clippy, typecheck, `cargo audit` (needs `cargo install cargo-audit --locked` and network access), unit tests, the coverage floor, and the GUI smoke capture. Do not commit `vault.cofferly`, `data.json`, or `.env` files.

If `cargo` is not on PATH on Windows, add `%USERPROFILE%\.cargo\bin` to PATH or run Cargo from that folder.

GitHub Actions CI is necessary but not sufficient: it does not run the full `scripts/verify.sh` gate (for example, no merge-blocking coverage floor or GUI smoke on every PR). CI runs `cargo test --locked` on Linux, macOS, and Windows. On **ubuntu-latest** only, the `Test` job also runs `scripts/check-vault-paths.sh`, `scripts/check-docs.sh`, `cargo fmt`, and `cargo clippy`. Separate jobs on **ubuntu-latest** run the coverage floor (`cargo llvm-cov`, job name **Coverage**) and `cargo audit` (job name **Security audit**). The **Coverage** job is informational and is not merge-blocking.

## Pull Request Checklist

- `scripts/verify.sh` passes locally.
- The app still opens to the Coffer Story (or Legacy PIN migration) unlock screen.
- Public files do not contain real child names or private family data.
- README or release docs are updated when behavior changes.

## Maintainer Control

Only maintainers decide whether a contribution fits Cofferly. A public repository does not mean every proposed change will be accepted.

