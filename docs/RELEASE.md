# Release Checklist

Use this checklist when preparing an AirWallet release.

## Local Checks

```powershell
C:\Users\danie\.cargo\bin\cargo.exe fmt -- --check
C:\Users\danie\.cargo\bin\cargo.exe test
C:\Users\danie\.cargo\bin\cargo.exe build --release
.\scripts\package-windows.ps1 -Version 0.1.0
```

The portable zip will be created in `dist/`.

## Windows Installer

The repository includes `installer/AirWallet.iss` for Inno Setup.

1. Install Inno Setup.
2. Build AirWallet with `cargo build --release`.
3. Open `installer/AirWallet.iss`.
4. Compile the installer.

The installer output is written to `dist/`.

## GitHub Release

1. Update `Cargo.toml`, `README.md`, and this checklist if the version changes.
2. Commit the release.
3. Tag it, for example `v0.1.0`.
4. Push the tag to GitHub.
5. Attach the generated portable zip and installer to the GitHub Release.

## Manual Smoke Test

Before publishing, open AirWallet and verify:

- The app launches as `AirWallet`.
- The parent PIN screen appears first.
- PIN `1234` unlocks a fresh install.
- Both default child wallets are visible.
- Adding a deposit changes the running balance.
- Adding a deduction changes the running balance.
- Print this ledger opens a printable browser page.
- Print both ledgers opens both ledgers in one printable page.
