# cofferly recovery runbook

This is the recovery runbook for cofferly: how to pull back a bad release and protect users' local vault data. Last verified 2026-09-27 (read-only). Anything not seen directly is marked **UNVERIFIED**. All RTOs are **estimates**.

---

## cofferly: Windows Rust/egui desktop app

- **Hosting:** none (no live service). Pushing a `v*` tag runs `release.yml` on `windows-latest`, which builds the portable zip plus the Inno Setup installer and publishes a GitHub Release.
- **Current state (verified):** one release, **v0.2.0 = Latest, and it has only `Cofferly-0.2.0-windows-x64.zip` (no `Setup.exe`)**. Tag `v0.1.0` exists, but its release doesn't, so **there's no prior release to point users back to**. PR #182 (per-wallet savings goal) is on `main` and unreleased.
- **Rollback of a bad release (RTO est. 1-2 min to stop new downloads, plus user reinstall time):**
```bash
gh release edit v0.3.0 -R danielendara/cofferly --prerelease --latest=false   # hide the bad one
gh release edit v0.2.0 -R danielendara/cofferly --latest                      # re-point Latest
# or: gh release delete v0.3.0 -R danielendara/cofferly --cleanup-tag
```
Docs: https://cli.github.com/manual/gh_release_edit. Then tell users to reinstall the previous installer.
- **Data:** local encrypted `vault.cofferly` in the user's app-data folder. In-app **Settings → Backup** makes a verified copy, and restore keeps `vault.pre-restore-<ts>.cofferly`. Risk: whether an older version can open a vault written by a newer one (e.g. with savings goals) is **UNVERIFIED**, so tell users to back up before a downgrade.
- **Health check:** `gh release view -R danielendara/cofferly --json tagName,assets --jq '.tagName, .assets[].name'` → the expected tag and assets. GitHub Actions runs on this repo.

### Known gaps (est.)

| Current RTO | Gap | Fix |
|---|---|---|
| 1-2 min to hide a release, **users must reinstall** | No prior release to fall back to. v0.2.0 has no installer. Vault downgrade compatibility unknown. | Keep ≥2 releases with installers; test that an older build opens a newer vault; release notes that tell users to back up |
