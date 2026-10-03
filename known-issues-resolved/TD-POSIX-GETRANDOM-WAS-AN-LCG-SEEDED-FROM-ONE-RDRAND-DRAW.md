### TD-POSIX-GETRANDOM-WAS-AN-LCG-SEEDED-FROM-ONE-RDRAND-DRAW. libc's CSPRNG was not one — 2026-08-12 — ✅ FIXED 2026-08-12 (`posix/src/random.rs`, `SYS_GETRANDOM`)

**What.** `getrandom()` and `getentropy()` — the two calls whose entire purpose
is to hand out key material, and which every ported crypto library reaches for
by name — were filled by `posix/src/unistd.rs::fill_random`, which was a
truncated LCG:

```rust
state = state.wrapping_mul(0x5851_F42D_4C95_7F2D)
             .wrapping_add(0x1405_7B7E_F767_814F);
*buf.add(i) = (state >> 56) as u8;
```

seeded from a *single* 64-bit `RDRAND` draw — or, when `RDRAND` failed, from
`SYS_CLOCK_MONOTONIC`. Its own doc comment said "Not cryptographically strong."

**Why it mattered.** Three separate failures, any one of which is fatal for the
API's contract:

1. **≤ 64 bits of entropy regardless of request size.** Asking for a 256-bit
   key returned 32 bytes stretched from one 64-bit seed.
2. **The state is recoverable from the output.** An LCG is affine; eight
   emitted bytes are eight windows onto `state`, and the recurrence is public.
   Anyone who sees a prefix of the stream can compute the rest — and, running
   the recurrence backwards, everything the process generated earlier.
3. **The fallback was a clock.** If `RDRAND` did not set CF, the seed was
   nanoseconds since boot: guessable to within a small window by anyone who
   knows roughly when the process started.

Meanwhile `kernel/src/rng.rs` had been a proper ChaCha20 CSPRNG all along —
seeded from RDRAND/RDSEED, TSC jitter, the HPET and interrupt-arrival timing —
with no syscall exposing it to userspace. The kernel had the right answer and
libc could not reach it.

`crt.rs`'s `AT_RANDOM` (16 bytes, which glibc and musl turn into the process's
stack canary) came from the same function, so stack-smashing protection was
built on the same LCG.

**Also fixed in passing.** `fill_random` issued `rdrand` unconditionally on
`target_arch = "x86_64"`, with no `CPUID` check. On a pre-2012 CPU that is
`#UD` — libc's random number generator would have crashed the process.

**Fix.** `posix/src/random.rs`:

* `SYS_GETRANDOM` (kernel-core 90) exposes `crate::rng::fill` to userspace,
  capped at 1 MiB per call and gated by `validate_user_write`.
* `getrandom`/`getentropy` go straight to that syscall.
* `arc4random`/`arc4random_buf`/`arc4random_uniform` run a per-thread ChaCha20
  pool with fast key erasure, seeded from the syscall (or from RDSEED/RDRAND
  behind a `CPUID` probe when the kernel is unreachable, which is how the host
  test build works), reseeded on `fork`.
* **No source, no bytes.** If neither the kernel nor a hardware RNG answers,
  `getrandom`/`getentropy` return `-1`/`EIO` and `arc4random`/`AT_RANDOM`
  abort. The old code's "always succeed, make something up" contract is what
  turned a missing entropy source into a silent security hole.

An RFC 8439 known-answer test pins the ChaCha20 core; see `design-decisions.md`
§117 for why the failure mode is fail-closed rather than best-effort.
