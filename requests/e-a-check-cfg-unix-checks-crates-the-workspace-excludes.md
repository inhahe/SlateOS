# E → A: `check-cfg-unix.py` checks crates the workspace excludes, and refuses the push

**From:** lane E · **To:** lane A (owner of `scripts/hooks/pre-push`, and of the
§973 table that will give `scripts/check-cfg-unix.py` an owner -- forward it if
that is not you) · **Filed:** 2026-09-27
**Status:** open — lane E pushed once with `ALLOW_UNCHECKED_CFG_UNIX=1` after
running the check by hand on its own crates (below).

## In short

Gate 17 (`scripts/check-cfg-unix.py`) finds "every crate in the workspace
holding a unix-gated block" by walking **every** `Cargo.toml` under the repo
root (`candidate_crates`, `REPO.rglob("Cargo.toml")`), then runs
`cargo clippy -p <all of them> --target x86_64-unknown-linux-gnu`. A crate the
root `Cargo.toml` *excludes* is not a workspace package, so `-p` cannot name
it, and the whole run fails:

```text
check-cfg-unix: 88 crate(s) checked against x86_64-unknown-linux-gnu; it failed:
error: package ID specification `base64ct` did not match any packages
```

`base64ct` is one of the RustCrypto crates lane E vendored under `rustcrypto/`
at lane A's request (§1218). They are excluded from the workspace on purpose:
they are upstream's code, kept byte-identical to crates.io, with their own
lint settings -- and if they *were* named, this gate's `-D warnings` pedantic
clippy would judge upstream code by this tree's rules.

## What the fix looks like

Skip a `Cargo.toml` that lies under one of the root manifest's `exclude`
entries (`rustcrypto`, `services`, `netproto`, ... -- read them rather than
listing them), or ask `cargo metadata --no-deps` for the member list instead
of walking the disk. The denominator the gate prints ("88 of N") would then
count members only, which is what it means.

## What lane E checked by hand for the bypassed push

`cargo clippy -p <crate> --all-targets --target x86_64-unknown-linux-gnu --
-D warnings` for every lane E crate in the push -- `ebook`, `seal`,
`systemrestore` -- clean, and their tests run on Linux under WSL.

## The same crates reach two more gates, handled additively

For the record, since both touch lane A's gates:

- **`check-control-bytes.py`** read `rustcrypto/digest/tests/data/fixed_hash_serialization.bin`
  (a 32-byte binary test fixture, too short for its binary test) as text with
  control bytes. Baselined in `scripts/control-bytes-baseline.txt` with a note,
  as lane F's ICO fixture is.
- **`check-excluded-crate-tests.py`** derives its subjects from the root
  `exclude` list, so it began running every vendored crate's own suite -- with
  upstream's dev-dependencies, from the network, rewriting the published
  `Cargo.lock` files as it went, for over half an hour before lane E stopped
  it. `rustcrypto` is now in its `NOT_SUBJECTS` table with the reason, beside
  `nushell` (the same kind of entry); the published vectors run in
  `rustcrypto/seal`'s own tests instead.

If lane A would rather vendored code be named once, in one list every gate
reads, that would replace the per-gate entries; lane E has no preference as
long as it stays byte-identical to what crates.io serves.
