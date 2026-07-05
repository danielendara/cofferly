# Changelog

## Unreleased

- Add encryption for the local data file using Argon2id key derivation + XChaCha20-Poly1305 authenticated encryption. Data is encrypted at rest with the parent PIN to prevent casual tampering. Old plain JSON files (including legacy imports) are automatically migrated on first successful unlock.
- Adds a Settings window for wallet management, parent PIN updates, starting balance edits, wallet deletion confirmation, and remove-entry undo.
- Splits rendering code into a dedicated views module and zeroizes derived keys plus plaintext serialization/decryption buffers when they are dropped.
- Refreshes README screenshots and updates documentation for the current Cofferly UI and security model.
- Renames the app to Cofferly after product-name collisions with previous AirWallet and Atlas Wallet names.
- Preserves automatic legacy data import for previous Atlas Wallet, TallyNest, and original AirWallet installs.
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
