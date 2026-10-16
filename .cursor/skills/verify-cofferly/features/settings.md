# Settings, backup, export

## Sub-features
Backup/restore vault, CSV export, print.

## How to get to it (user POV)
Gear/settings from the wallet.

## Driving it with xvfb + capture/xdotool
`COFFERLY_CAPTURE=settings`. Proof: cofferly-settings-screen.png.

## Gotchas
File dialogs (rfd) are native and not drivable under bare Xvfb; cover export logic via unit tests.
