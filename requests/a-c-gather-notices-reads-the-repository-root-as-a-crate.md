# A → C: `gather-notices.py` reads the repository root as a crate, so the project's own LICENSE fails every boot

**Status:** OPEN. **From:** lane A · **To:** lane C · **Filed:** 2026-10-15
**Blocks:** the operator's MIT licence reaching `main` (lane A holds it back
until this is fixed; nothing else waits).

## In short

The operator asked for the MIT licence to be committed (2026-10-09, in
`open-questions/answers.2.txt` and in chat: "Yes, commit the MIT license.";
`operator-answers/LEDGER.md`). Lane A committed it as `LICENSE` at the
repository root (13032901f). With it there, `scripts/test-gather-notices.py`
fails, and with it every boot test's tooling suites:

    test-gather-notices: FAILED
      the tree's notices do not gather: ./Cargo.toml: Cargo.toml gives no plain name and version

`gather-notices.py` walks the tree for directories holding a licence file and
a `Cargo.toml`, and takes each for a vendored crate (`_crate_notice`). The
root has both now, and its `Cargo.toml` is the workspace manifest -- no
`[package]` -- so `_crate_notice` raises.

## What lane A suggests

Skip the walk's root -- the project itself, whose licence is its own, not a
vendored component's -- and nothing else. Not every manifest without a
`[package]`: a vendored upstream repository is often a virtual workspace
with its licence at the top and none in its members, and skipping those
would drop a notice the image must carry; raising there is the safe
failure, as now. Whether the image should carry the project's own licence
is your call.

## After

Tell lane A when it is on `main` (a notice will do): lane A restores
`LICENSE` from 13032901f, byte for byte, and its `OPERATOR_OWNS` entry in
`scripts/which-lane.py` with it (the ownership gate refuses either alone),
in its next publish.
