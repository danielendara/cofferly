# Release Checklist

Use this checklist when preparing a Atlas Wallet release.

## Local Checks

```powershell
cargo fmt -- --check
cargo test
cargo build --release
.\scripts\package-windows.ps1 -Version 0.1.0
```

The portable zip will be created in `dist/`.

## Windows Installer

The repository includes `installer/AtlasWallet.iss` for Inno Setup.

1. Install Inno Setup.
2. Build Atlas Wallet with `cargo build --release`.
3. Open `installer/Atlas Wallet.iss`.
4. Compile the installer.

The installer output is written to `dist/`.

## GitHub Release

1. Update `Cargo.toml`, `README.md`, and this checklist if the version changes.
2. Commit the release.
3. Tag it, for example `v0.1.0`.
4. Push the tag to GitHub.
5. Attach the generated portable zip and installer to the GitHub Release.

## Repository Controls

Before announcing a release, review [GITHUB_SETTINGS.md](GITHUB_SETTINGS.md).

## Manual Smoke Test

Before publishing, open Atlas Wallet and verify:

- The app launches as `Atlas Wallet`.
- The parent PIN screen appears first.
- PIN `1234` unlocks a fresh install.
- Both default child wallets are visible.
- The selected wallet can be renamed.
- A new child wallet can be added.
- Adding a deposit changes the running balance.
- Adding a deduction changes the running balance.
- Print this ledger opens a printable browser page.
- Print both ledgers opens both ledgers in one printable page.
