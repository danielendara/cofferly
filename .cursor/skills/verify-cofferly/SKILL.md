---
name: verify-cofferly
description: Launch and drive the Cofferly desktop app (Rust/egui, encrypted local vault) headlessly under Xvfb, capture screenshots and transcripts as proof. Use when a change must be shown working in the real app, or to run the full local verify gate.
---

# verify-cofferly

Surface: native desktop GUI (eframe/egui, wgpu). No server, no ports, no network. Data lives in `$COFFERLY_DATA_DIR/Cofferly/`. CI is local-only: the gate is `scripts/verify.sh`.

## Full gate
`scripts/verify.sh` — vault guard, fmt, clippy, check, tests, coverage (>=74%), GUI smoke. Fails fast. Needs `cargo-llvm-cov`, `xvfb-run`. If `cargo` has no default toolchain: `export RUSTUP_TOOLCHAIN=<installed toolchain>`.

## Launch
```bash
cargo build --release --locked            # -> target/release/Cofferly
D=$(mktemp -d)                            # empty data dir = first-run Coffer Story setup, no seed needed
xvfb-run -a -s "-screen 0 1280x800x24" bash -c \
  "COFFERLY_DATA_DIR=$D target/release/Cofferly 2>$EVID/live-stderr.log & P=\$!; sleep 12; import -window root $EVID/live-first-launch.png; kill \$P"
```
Ready = a window titled `Cofferly` exists (`xdotool search --name Cofferly`); allow ~5-12 s (software rendering). ALWAYS set `COFFERLY_DATA_DIR`; never point at a real vault.

### Headless libs (only if the app panics `libxkbcommon-x11.so could not be loaded` or `Wgpu(CreateSurfaceError…)`)
Need `libxkbcommon-x11-0`, `libxcb-xkb1`, and a Vulkan ICD (`mesa-vulkan-drivers`, lavapipe). Without root: `apt-get download` them, `dpkg -x` into a scratch dir `$X` (not the repo), then
`export LD_LIBRARY_PATH=$X/usr/lib/x86_64-linux-gnu VK_ICD_FILENAMES=$X/usr/share/vulkan/icd.d/lvp_icd.json` (edit `library_path` in that json to the absolute `.so` path). This is verification scaffolding; it is not part of the repo.

## Doctor (read-only)
`test -x target/release/Cofferly && xvfb-run -a true && echo ok`; then a capture run (below) exiting 0 with a PNG proves the render stack works. Check `git rev-parse HEAD` matches the build you are proving (rebuild if not).

## Drive
Deterministic screens use the app's own capture mode (one target per process, requires `COFFERLY_DATA_DIR`, builds a demo vault in it):
```bash
COFFERLY_DATA_DIR=$D COFFERLY_CAPTURE=wallet COFFERLY_CAPTURE_DIR=$EVID \
  xvfb-run -a -s "-screen 0 1280x900x24" timeout 150 target/release/Cofferly
```
Targets: `story-unlock`, `wallet`, `settings`, `story-setup`. Files: `cofferly-<target>.png`. For real interaction on the live window use `xdotool` (`mousemove/click/type`) against `DISPLAY` of the Xvfb, then `import -window root`; egui exposes no DOM/ARIA handles, so locate widgets from screenshots (window is min 820x560). Feature journeys: `features/README.md`.

## Evidence
Save under `$EVID` (default `/workspace/verify-<date>/cofferly-evidence/`, never in the repo): before/after PNGs, stderr log, capture transcript, data-dir listing. Proof = real user path (live launch or capture of the real UI code), the action and the resulting state, plus side effects (vault file present in data dir). Don't print story words or PINs from screenshots into PR text beyond the throwaway-demo ones; captures use demo data only.

## Isolation
Fresh `mktemp -d` data dir and `xvfb-run -a` (auto display number) per run, so instances can run side by side. Never launch against the user's real data dir or real display.

## Cleanup
`kill` the PIDs you started (`$P`), `rm -rf "$D"`. Never `pkill Cofferly`. Leave `$EVID` intact.
