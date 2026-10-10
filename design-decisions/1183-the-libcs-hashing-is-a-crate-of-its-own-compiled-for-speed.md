## 1183. The libc's hashing is a crate of its own, compiled for speed

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** the C library is compiled to be small rather than fast,
because every program carries a copy of the parts it uses (§100). For
password hashing that was the wrong trade: checking a password took up to
3.7 times as long as Ubuntu's library took for the same check. The hashing
code -- SHA-2, MD5, the rounds of the `$5$`, `$6$` and `$1$` methods, and
yescrypt -- now lives in a small library of its own inside the C library,
`posix/pwhash`, which is compiled for speed. Logins hash as fast as
Ubuntu's, and the rest of the C library is exactly as small as it was.

**The choice:**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. The hashing in its own crate at opt-level 3** (chosen) | `crypt` at or below libxcrypt's time; the libc's other code unchanged | the speed where it is wanted and nowhere else; §100's instrument, which the kernel, the shell and the codecs already use | a crate boundary inside `posix`: its memory must be passed in (the crate has no allocator), and nothing hot may be `#[inline]` or generic across it |
| B. The whole libc at opt-level 2 or 3 | every libc function faster; `crypt` as A | one line | the libc's code 38% larger at 3 (host staticlib `.text`, 1.90 to 2.61 MB), in every program, for functions few of them run hot |
| C. libxcrypt's SSE2 code, ported | none beyond A | what Ubuntu runs | at opt-level 3 the portable code already matches it; at `s` the SSE2 code would be held back the same way |
| D. Nightly's `#[optimize(speed)]` on the hot functions | as A, inside `posix` | no new crate | an unstable attribute, and the host tests and benches build with stable |

**Why A.** The cost was concentrated: a few hundred lines of arithmetic,
every one of them on the hot path of every password check. B spends size in
every program to buy speed in one; C changes the code when the code was not
the cause; D builds on an attribute that may change and that half the
builds cannot use.

**When to revisit:** if a profile shows other libc code -- `printf`,
`strtod`, `qsort`, regex, the allocator -- hot in real programs, measure B
for the whole library; if `#[optimize]` stabilises, D could fold the crate
back into `posix`.

**Where:** `posix/pwhash/` (its crate docs say what is in it and why); the
root `Cargo.toml`'s `[profile.release.package.pwhash]` and
`[profile.dev.package.pwhash]`; `posix/src/crypt.rs` and
`posix/src/yescrypt.rs`, which keep the settings, the C ABI and the memory;
measured by `posix/benches/crypt.rs` against `libxcrypt-reference.c`.
