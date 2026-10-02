## §94 — osh `$SRANDOM`: read `/dev/urandom` per expansion, with a documented non-cryptographic fallback

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** bash 5.1 added `$SRANDOM`: 32 bits drawn from the *system
entropy source* rather than from bash's own `$RANDOM` LCG, with no seed (so
assignments to it are documented as having no effect). Scripts reach for it
precisely when `$RANDOM` is too weak — temp-file names, tokens, salts. osh had
no `$SRANDOM` at all, so `$SRANDOM` expanded to the empty string.

Implementing it forces a choice about what to do on a build where there is no
entropy device — notably the *host* (Windows) build of osh, which is how the
shell is developed and tested.

**Options.**

* **A — entropy device only; expand to empty (or fail) without one.** Never
  hands out a weak number. But it makes `$SRANDOM` silently useless on the host
  build, and "empty" is the worst possible failure for `salt=$SRANDOM`: under
  `set -u` it aborts, and without it the script proceeds with an empty salt.
* **B — pull in the `getrandom` crate.** Real per-OS backends (BCrypt on
  Windows, the `getrandom` syscall on Linux). But it has no backend for the
  custom `x86_64-slateos` target, so it would need the custom-backend hook
  wired up anyway — a dependency plus a shim, to serve a path that on the real
  target is just `/dev/urandom`.
* **C (chosen) — `/dev/urandom` per expansion, falling back to a SplitMix64
  stream seeded from clock+pid.** On SlateOS the device is the kernel CSPRNG
  behind `getrandom`, which is exactly bash's source; the fallback only ever
  runs where there is no device.

**Why C.** It is what bash itself does — `variables.c` falls back to bash's own
generator when no entropy source is available — and it is what the rest of this
userspace already does (`mktemp`, `passwd`, `useradm` all read `/dev/urandom`
with a PRNG fallback). Adding a dependency (B) to serve a case the target does
not have is cost without benefit, and A trades a weak number for an *empty* one,
which is not safer.

**What keeps the fallback honest.** It is a separate stream from `$RANDOM`, so
`RANDOM=1` cannot make `$SRANDOM` reproducible; it is seeded from clock+pid, so
two shells started in the same microsecond diverge; a subshell perturbs its
state *and* advances the parent's, so neither a subshell and its parent nor two
subshells started in the same instant repeat each other's numbers; and the
device is probed once — after the first failure the flag is cleared, so a host
build does not pay a failed `open` per expansion. The fallback's
non-cryptographic status is stated at the function, not buried.

**How to reverse.** `Shell::next_srandom` is the only reader; swapping in a
different source (a crate, a syscall wrapper, a kernel-provided handle) is a
change to that one function and to `read_entropy_u32`.
