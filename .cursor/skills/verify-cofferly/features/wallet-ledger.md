# Wallet and ledger

## Sub-features
Balances, deposits/spends, weekly allowance, correction form with Undo.

## How to get to it (user POV)
Unlock, then the wallet screen.

## Driving it with xvfb + capture/xdotool
`COFFERLY_CAPTURE=wallet` (demo vault). Interactive edits: xdotool on the live window after unlocking.

## Gotchas
Capture mode seeds demo data only; interactive journeys need the demo story words from src/capture.rs (DEMO_STORY).
