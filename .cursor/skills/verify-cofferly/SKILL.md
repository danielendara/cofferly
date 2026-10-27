---
name: verify-cofferly
description: Launch and drive the Cofferly desktop app (Rust/egui, encrypted local vault) headlessly under Xvfb, capture screenshots and transcripts as proof. Use when a change must be shown working in the real app, or to run the full local verify gate.
---

# verify-cofferly

Surface: native desktop GUI (eframe/egui, wgpu). No server, no ports, no network. Data lives in `$COFFERLY_DATA_DIR/Cofferly/`. GitHub Actions CI (`.github/workflows/ci.yml`) is live; run `scripts/verify.sh` locally before every PR and make sure CI is green too.

## Full gate
`scripts/verify.sh` — vault guard, docs self-check, fmt, clippy, check, **cargo audit**, tests, coverage (>=74%), GUI smoke. Fails fast. Needs `cargo-audit` (`cargo install cargo-audit --locked`), `cargo-llvm-cov`, `xvfb-run`. The audit step needs network (RustSec advisory DB). If audit fails on a new advisory, update dependencies first; only then add a `RUSTSEC-…` id to `.cargo/audit.toml` (same list CI uses). If `cargo` has no default toolchain: `export RUSTUP_TOOLCHAIN=<installed toolchain>`.

## Launch
```bash
EVID=${EVID:-/workspace/verify-$(date +%F)/cofferly-evidence}; mkdir -p "$EVID"
cargo build --release --locked            # -> target/release/Cofferly
D=$(mktemp -d)                            # empty data dir = first-run Coffer Story setup, no seed needed
# setsid makes the launcher its own process group leader: PGID == its PID, so cleanup is `kill -- -PGID`.
COFFERLY_DATA_DIR=$D setsid xvfb-run -a -s "-screen 0 1280x800x24" bash -c \
  "target/release/Cofferly 2>$EVID/live-stderr.log & sleep 12; import -window root $EVID/live-first-launch.png; wait" &
PGID=$!; echo "PGID=$PGID"
sleep 16; ls -l "$EVID/live-first-launch.png"
```
Ready = a window titled `Cofferly` exists (`xdotool search --name Cofferly`); allow ~5-12 s (software rendering). ALWAYS set `COFFERLY_DATA_DIR`; never point at a real vault.

### Headless libs (only if the app panics `libxkbcommon-x11.so could not be loaded` or `Wgpu(CreateSurfaceError…)`)
Need `libxkbcommon-x11-0`, `libxcb-xkb1`, and a Vulkan ICD (`mesa-vulkan-drivers`, lavapipe). Without root: `apt-get download` them, `dpkg -x` into a scratch dir `$X` (not the repo), then
`export LD_LIBRARY_PATH=$X/usr/lib/x86_64-linux-gnu VK_ICD_FILENAMES=$X/usr/share/vulkan/icd.d/lvp_icd.json` (edit `library_path` in that json to the absolute `.so` path). This is verification scaffolding; it is not part of the repo.

## Doctor (read-only)
`test -x target/release/Cofferly && xvfb-run -a true && echo ok`; then a capture run (below) exiting 0 with a PNG proves the render stack works. Check `git rev-parse HEAD` matches the build you are proving (rebuild if not).

## Drive
### Drive → headless (no display, GPU or xdotool)
`cargo test ui_tests -- --nocapture` drives the real `eframe::App::update` through `egui_kittest` (AccessKit): widgets are found by role and accessible label (for example `Record spending`, `Lock`, `Coffer Story object: <name>`) and by key events, never by coordinates. `src/ui_tests.rs` builds the app with `CofferlyApp::new` against an empty temp `COFFERLY_DATA_DIR` (hold a `ScopedDataDir`). Add a journey there for UI-state behaviour before reaching for the xdotool recipe below.

Deterministic screens use the app's own capture mode (one target per process, requires `COFFERLY_DATA_DIR`, builds a demo vault in it):
```bash
COFFERLY_DATA_DIR=$D COFFERLY_CAPTURE=wallet COFFERLY_CAPTURE_DIR=$EVID \
  xvfb-run -a -s "-screen 0 1280x900x24" timeout 150 target/release/Cofferly
```
Targets: `story-unlock`, `wallet`, `settings`, `story-setup`. Files written to `$COFFERLY_CAPTURE_DIR` (exactly `CaptureTarget::file_name` in `src/capture.rs`): `cofferly-story-unlock.png`, `cofferly-wallet-screen.png`, `cofferly-settings-screen.png`, `cofferly-story-setup.png`; `scripts/check-docs.sh` fails if this list drifts. Prefer `cargo test ui_tests` (egui_kittest / AccessKit in `src/ui_tests.rs`) for deterministic UI drives; use `xdotool` (`mousemove/click/type`) on the live Xvfb window only as a fallback, then `import -window root` (window is min 820x560). Feature journeys: `features/README.md`.

## Evidence
Save under `$EVID` (default `/workspace/verify-<date>/cofferly-evidence/`, never in the repo): before/after PNGs, stderr log, capture transcript, data-dir listing. Proof = real user path (live launch or capture of the real UI code), the action and the resulting state, plus side effects (vault file present in data dir). Don't print story words or PINs from screenshots into PR text beyond the throwaway-demo ones; captures use demo data only.

## Isolation
Fresh `mktemp -d` data dir and `xvfb-run -a` (auto display number) per run, so instances can run side by side. Never launch against the user's real data dir or real display.

## Cleanup
Kill the process group by PGID (never `pkill -f`/`pkill Cofferly`), remove the temp dir, then ASSERT it is gone. The app opens no ports; the check proves nothing of ours (app or Xvfb) is listening and the launcher group is dead. Leave `$EVID` intact.
```bash
kill -- -"$PGID" 2>/dev/null; sleep 2; kill -9 -- -"$PGID" 2>/dev/null; rm -rf "$D"
if ps -o pid= -g "$PGID" | grep -q .; then echo "FAIL: process group $PGID still alive"; false; fi
if ss -ltnp | grep -E 'Cofferly|Xvfb'; then echo "FAIL: listener still up"; false; fi
echo "cleanup OK: group $PGID gone, no Cofferly/Xvfb listeners"
```
