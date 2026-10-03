## §117 — Userspace randomness comes from the kernel, and fails closed

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Where:** `posix/src/random.rs` (new), `kernel/src/syscall/number.rs`
(`SYS_GETRANDOM` = 90), `kernel/src/syscall/handlers.rs::sys_getrandom`,
`posix/src/unistd.rs::{fill_random, getrandom, getentropy}`,
`posix/src/crt.rs::ensure_at_random_initialized`,
`posix/src/perthread.rs::PerThread::random`, `posix/src/process.rs::fork`.

### The problem

`getrandom()`/`getentropy()` were backed by a 64-bit LCG seeded from one
`RDRAND` draw, falling back to the monotonic clock. See
`known-issues.md` `TD-POSIX-GETRANDOM-WAS-AN-LCG-SEEDED-FROM-ONE-RDRAND-DRAW`
for why each of those three properties is independently fatal. The kernel
already ran a real ChaCha20 CSPRNG (`kernel/src/rng.rs`) with four entropy
sources; nothing exposed it.

Three decisions had to be made, and none of them is forced.

### 1. `SYS_GETRANDOM` is not capability-gated

Every other resource-touching syscall in this kernel goes through
`require_cap_type`. Randomness does not, for three reasons: there is no
resource another process can be deprived of (the CSPRNG is stateless from the
caller's point of view), there is nothing to authorise reading (the output is
by construction independent of anything the caller could otherwise not see),
and refusing it would push callers back onto a homebrew PRNG — which is the bug
we just fixed, re-created one library at a time. Linux, the BSDs and Fuchsia
(`zx_cprng_draw`) all reach the same conclusion. The one genuine cost is CPU
time, which the scheduler already bounds.

**Against:** it is a real exception to "no ambient authority", and a sandbox
that wants to make a process fully deterministic (for record/replay debugging)
now cannot. If that use case appears, the right answer is a per-process
*determinism* mode that seeds the pool from a fixture, not a capability.

### 2. No entropy source means failure, not best-effort bytes

If neither the kernel CSPRNG nor a hardware RNG answers, `getrandom`/
`getentropy` return `-1`/`EIO`, and `arc4random` and `AT_RANDOM` — which have
no error channel — abort the process.

**For:** this is precisely the bug being fixed. The old code's contract was
"always succeed", so a missing entropy source degraded silently into
predictable output that callers spent as if it were secret. A caller handed
`EIO` can fail closed; a caller handed predictable bytes cannot do anything at
all. OpenBSD takes the same position (its `arc4random` raises `SIGKILL`).

**Against:** a program that only wanted a temp-file suffix now dies where it
used to muddle through, and `getrandom` gains a failure mode Linux does not
have (Linux's pool is always seedable, so there is no ABI precedent for `EIO`
here — `getentropy`'s specified `EIO` is the closest thing, which is why both
calls report it). Accepted: on the OS target this can only fire if the kernel
is broken, and a broken kernel should not be papered over.

`GRND_INSECURE` deliberately does **not** override this. That flag means "do
not block waiting for the pool to initialise", not "make something up".

### 3. `arc4random` gets a userspace pool; `getrandom` does not

`arc4random` is expected to be cheap enough to call in a loop, so a syscall per
call would defeat it: it runs a ChaCha20 stream in userspace, seeded from the
kernel. `getrandom` goes straight to the kernel every time, matching Linux —
a caller asking for key material *by name* should not be served from a
userspace copy that outlives the call in this process's address space.

The pool lives in `PerThread`, so it needs no lock and two threads can never be
handed the same bytes; and it uses **fast key erasure** (256 bytes of keystream
per refill, the first 32 of which immediately replace the key and are never
emitted). The 12.5% throughput cost buys forward secrecy: an attacker who reads
the pool later cannot roll it backwards to bytes the process already used.

**The fork hazard.** A forked child inherits a byte-identical pool and would
replay the parent's stream, so a parent and child that each generate a "random"
session key generate the *same* one. `fork()`'s child branch bumps a
process-wide generation counter that invalidates every thread's pool.

*Alternative rejected:* comparing a cached pid on each call (LibreSSL's
approach). That is a syscall per `arc4random()`, which is the cost we built the
pool to avoid. *Also rejected:* `MADV_WIPEONFORK` on the pool page (glibc's and
OpenBSD's approach) — we have no such madvise flag, and adding one to serve a
single caller is a worse trade than a relaxed atomic load.

### Consequences

- `posix::random::secure_bytes` is now the single source of random bytes in
  libc. Anything that needs randomness — TLS, `mkstemp`, ASLR seeds, hash-table
  seeds — should call it rather than growing its own.
- `fill_random` returns `bool` and is `unsafe`; every caller must handle
  failure. There is deliberately no infallible variant.
- The host build (`cargo test` against the Windows triple) reaches the pool via
  RDSEED/RDRAND, because the raw `SYSCALL` instruction is gated off there. That
  path is cryptographically sound, not a stub — which matters, because it is
  the path all the unit tests exercise.
- `CPUID` is now probed before `RDRAND`/`RDSEED` is issued. The old code issued
  `rdrand` unconditionally, which is `#UD` on a pre-2012 CPU.
