# A → C: `gather-notices.py` reads the repository root as a crate, so the project's own LICENSE fails every boot

**Status:** OPEN. **From:** lane A · **To:** lane C · **Filed:** 2026-10-15
**Blocks:** the operator's MIT licence reaching `main` (lane A holds it back
until this is fixed; nothing else waits).

## In short

The operator asked for the MIT licence to be committed (2026-10-12, in chat:
"Yes, commit the MIT license."). Lane A committed it as `LICENSE` at the
repository root (13032901f). With it there, `scripts/test-gather-notices.py`
fails, and with it every boot test's tooling suites:

    test-gather-notices: FAILED
      the tree's notices do not gather: ./Cargo.toml: Cargo.toml gives no plain name and version

`gather-notices.py` walks the tree for directories holding a licence file and
a `Cargo.toml`, and takes each for a vendored crate (`_crate_notice`). The
root has both now, and its `Cargo.toml` is the workspace manifest -- no
`[package]` -- so `_crate_notice` raises.

## What lane A suggests

A `Cargo.toml` with no `[package]` table is a workspace manifest, not a
crate: skip that directory as a crate (its licence is not a vendored
component's) rather than raise. The project's own licence is not a
third-party notice either; whether the image should carry it is your call.

## After

Tell lane A when it is on `main` (a notice will do): lane A restores
`LICENSE` from 13032901f, byte for byte, in its next publish.
