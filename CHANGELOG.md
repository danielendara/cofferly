# Changelog

## Unreleased

- Renames TallyNest to Atlas Wallet (chosen by the kids for the wallet analogy).
- Preserves automatic legacy data import for previous TallyNest installs and original AirWallet data.
- Updates all references in code, docs, build scripts, installer, tests, screenshots, and GitHub workflows.

## 0.1.0

- Renames AirWallet to TallyNest to avoid confusion with an existing payment platform.
- Imports existing AirWallet data automatically on first launch.
- Initial TallyNest desktop app.
- Adds two default child wallets.
- Tracks deposits, deductions, and running balances.
- Adds parent PIN unlock with first-run PIN `1234`.
- Adds child wallet renaming and adding custom child wallets.
- Adds printable ledger export for one wallet or both wallets.
- Adds Windows portable packaging script.
- Adds Inno Setup installer script.
- Adds GitHub Actions release workflow.
- Updates GitHub Actions and Rust dependencies after first Dependabot scan.
