# Security Policy

## Supported Versions

Cofferly is early-stage software. Security fixes target the latest released version.

## Reporting a Vulnerability

Please do not open a public issue for vulnerabilities that could expose private family data or weaken parent-mode behavior.

Report security concerns privately through GitHub's private vulnerability reporting if enabled, or contact the maintainer directly through GitHub.

## Security Scope

Cofferly uses a simple 4-digit parent PIN as both a family-use editing lock and the input for local data-file encryption. It should not be treated as strong security.

Private ledger data is stored locally on the user's machine.

### Data at rest

Ledger data is encrypted at rest with XChaCha20-Poly1305 (authenticated encryption). The 32-byte key is derived from the parent PIN using Argon2id (64 MiB memory, 3 iterations, 1 parallelism lane). The salt and nonce are random per file and stored alongside the ciphertext. Derived keys and plaintext serialization/decryption buffers are zeroized when dropped.

### PIN brute-force (intentional trade-off)

There is **no software lockout** on wrong PIN attempts. This is intentional for a family app: a parent must always be able to unlock, and a lockout could lock a family out of their own data.

The Argon2id key derivation acts as a deliberate rate limiter: each attempt costs roughly tens of milliseconds of CPU and 64 MiB of memory, so guessing all 10,000 four-digit combinations is slow and impractical on a casual machine. This matches the stated threat model: a "kid-proof" lock, not resistance against a determined attacker with the data file. If the data file is exfiltrated, an offline attacker with the PIN's small keyspace can eventually brute-force it.
