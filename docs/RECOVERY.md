# cofferly recovery runbook

This is the recovery runbook for cofferly: how to pull back a bad release and protect users' local vault data. Anything not seen directly is marked **UNVERIFIED**. All RTOs are **estimates**.

For what is tagged, published, and built today, see [RELEASE.md](RELEASE.md) and [GitHub Releases](https://github.com/danielendara/cofferly/releases) (`gh release list -R danielendara/cofferly`). The app version on `main` is in `Cargo.toml`.

---

## cofferly: Windows Rust/egui desktop app

- **Hosting:** none (no live service). Pushing a `v*` tag runs `release.yml` on `windows-latest`, which builds the portable zip plus the Inno Setup installer and publishes a GitHub Release. The workflow can also be started manually via `workflow_dispatch` (build artifacts only, no Release attach; version comes from `Cargo.toml`).
- **Rollback of a bad release (RTO est. 1–2 min to stop new downloads, plus user reinstall time):**
```bash
gh release edit v0.4.0 -R danielendara/cofferly --prerelease --latest=false   # hide the bad one
gh release edit v0.2.0 -R danielendara/cofferly --latest                      # re-point Latest
# or: gh release delete v0.4.0 -R danielendara/cofferly --cleanup-tag
```
Docs: https://cli.github.com/manual/gh_release_edit. Then tell users to reinstall the previous good build (check GitHub **Latest** for the current download).
- **Data:** local encrypted `vault.cofferly` in the user's app-data folder. In-app **Settings → Backup** makes a verified copy, and restore keeps `vault.pre-restore-<ts>.cofferly`. Risk: whether an older build can open a vault written by a newer one (for example after savings goals or weekly allowance) is **UNVERIFIED**, so tell users to back up before a downgrade.
- **Health check:** `gh release view -R danielendara/cofferly --json tagName,assets --jq '.tagName, .assets[].name'` → the expected tag and assets. GitHub Actions runs on this repo.

### Known gaps (est.)

| Current RTO | Gap | Fix |
|---|---|---|
| 1–2 min to hide a release; users must reinstall | Confirm each release ships zip + `Setup.exe`; vault downgrade compatibility unknown. | Publish ≥2 releases with zip + `Setup.exe`; test that an older build opens a newer vault; release notes that tell users to back up |
