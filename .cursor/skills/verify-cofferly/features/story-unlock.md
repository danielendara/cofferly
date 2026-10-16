# Story unlock

## Sub-features
Pick six story objects to unlock; Legacy PIN migration path.

## How to get to it (user POV)
Launch with an existing vault.

## Driving it with xvfb + capture/xdotool
`COFFERLY_CAPTURE=story-unlock`. Proof: cofferly-story-unlock.png shows the unlock grid.

## Gotchas
Argon2 prep is heavy; one capture target per process.
