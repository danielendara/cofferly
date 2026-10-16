# First-run Coffer Story setup

## Sub-features
Generate story, regenerate, print recovery card, restore from backup.

## How to get to it (user POV)
Launch the app with no vault; the setup card appears.

## Driving it with xvfb + capture/xdotool
Live launch with empty `COFFERLY_DATA_DIR`, wait ~12 s, `import -window root`. Or `COFFERLY_CAPTURE=story-setup`. Proof: screenshot shows "Your new Coffer Story" with six numbered objects and 'I wrote it down — continue'. Verified in this repo's skill run.

## Gotchas
Story is random per launch (throwaway). Print/backup open native dialogs; not drivable headless.
