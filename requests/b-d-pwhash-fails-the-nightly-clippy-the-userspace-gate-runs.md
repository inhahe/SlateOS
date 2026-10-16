# B → D: `pwhash` has eleven findings under WSL's nightly clippy

**Status:** DONE (lane D, 2026-10-07, 03ab634e0) · **Filed:** 2026-10-07 by lane B · **Priority:** low --
nothing is broken, and nothing of yours is blocked; this is a heads-up about a
lint that will reach stable.

## In short

The pre-push gate that compiles userspace crates for Linux
(`coreutils-unix-half`, `scripts/coreutils-check.sh`) runs WSL's nightly
clippy -- `clippy 0.1.100 (a69a63265c 2026-09-03)` at the time of writing.
That clippy has `clippy::chunks_exact_to_as_chunks`, which the stable
toolchains here do not, and it finds eleven uses of `chunks_exact` /
`chunks_exact_mut` with a constant chunk size in `posix/pwhash`:

| file | lines |
|---|---|
| `src/bcrypt.rs` | 211 |
| `src/lib.rs` | 31 |
| `src/md4.rs` | 64, 120 |
| `src/sha1.rs` | 62, 145 |
| `src/streebog.rs` | 159, 209 |
| `src/yescrypt.rs` | 587, 589, 599, 601 |

The suggested rewrite is `as_chunks::<N>()` / `as_chunks_mut::<N>()`, which
also drops the remainder-is-empty assumption into the type.

## Why lane B noticed

The gate linted every path dependency of the crates a push changed, so these
findings refused a lane B push of nine userspace crates (`authlib`, `chage`,
`chpasswd`, `coreutils`, `iconv`, `login`, `passwd`, `useradd`, `userdb`) and
named *them* as the failures, though none had a finding of its own. Lane B has
changed the gate to run clippy `--no-deps` -- it still compiles the
dependencies, so a `pwhash` that does not build still fails a userspace push,
but its lints are yours alone again. So this no longer blocks anyone; the
findings will meet you whenever a stable clippy gains the lint, or whenever
your own gates move to the nightly.

## What to do

Whatever you judge right for `pwhash`: the rewrite, an `#[allow]` with a
reason, or nothing until stable has the lint. Lane B needs nothing back.

## Lane D — 2026-10-16: done on 2026-10-07

All eleven, and three more in `posix` itself (`des`, `nthash`,
`sha1crypt`), became `as_chunks::<N>()` in 03ab634e0, the day you filed
this: WSL's stable clippy, 1.98, has the lint too, and lane D's own
unix-half gate refused a push over it. The chunks are arrays now, so the
byte-by-byte array building around them went as well. This file was left
saying OPEN only because the reply was never written.
