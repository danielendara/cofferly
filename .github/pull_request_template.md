## Purpose
- Why: <one plain sentence; may carry over from the issue>
- Value: High | Medium | Low — <what we gain or avoid>
- Priority: P1 | P2 | P3
- Outcome achieved: <past tense, plain words>
- Human-readable proof: <what a person can now see or check: screenshot, header, short live check>
- Risk and rollback: <revert this PR, or migration/undo note>

## Verification
<!-- Paste `scripts/verify.sh` tail and one mutation proof. State "No migration" or "MIGRATION FIRST" (with the exact step). -->

## Summary

Describe the change and why it belongs in Cofferly.

## Checklist

- [ ] I ran [`scripts/verify.sh`](../scripts/verify.sh) locally (see [CONTRIBUTING.md](../CONTRIBUTING.md#local-development)); green CI is necessary but not sufficient.
- [ ] I did not include real child names, private ledger data, or family screenshots.
- [ ] I updated docs if behavior changed.

## Notes

Anything the maintainer should pay special attention to?
