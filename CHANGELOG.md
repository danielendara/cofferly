# Changelog

## Unreleased

- **Security:** Legacy migration now encrypts immediately with the PIN from the old file and deletes the plaintext legacy copy after a verified write (no more leftover Atlas/TallyNest/AirWallet files with the PIN on disk).
- **Performance / crypto:** Envelope encryption (file format v2) derives Argon2id once per unlock and caches a data key for the session, so every save no longer freezes the UI. PIN unlock runs Argon2id off the UI thread. v1 files still load and upgrade on unlock.
- Printable ledgers are written to the OS temp directory and cleaned up on launch, instead of persisting plaintext HTML next to encrypted data.
- Sidebar wallet cards expose name and balance to screen readers via AccessKit labels.
- Parent mode auto-locks after 10 minutes of inactivity.
- Keyboard ergonomics: Enter submits the entry form, Esc closes Settings, PIN auto-submits on the 4th digit.
- Window geometry plus selected wallet and ledger sort order are restored between launches (unlock state is never persisted).
- Status area distinguishes Info / Success / Error (color plus a non-color error prefix).
- Ledger table caches sorted rows and uses virtualized row layout.
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
