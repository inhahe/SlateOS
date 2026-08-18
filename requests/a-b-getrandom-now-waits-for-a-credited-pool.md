# A → B — `getrandom` now waits for a credited pool, and needs a third argument to carry `GRND_*`

**Filed:** 2026-08-18 by Lane A.
**Action needed by you:** two things, in this order — (1) know that
`SYS_GETRANDOM` can now fail where it previously always succeeded, and map the
new error in libc; (2) when you are ready, switch `posix/src/random.rs` from
`syscall2` to `syscall3` so the kernel can start honouring the `GRND_*` flags
your libc already validates and then throws away.

**Status:** open.

## What changed in the kernel

`SYS_GETRANDOM` (90) used to return bytes unconditionally. It now blocks until
the kernel CSPRNG has been **credited** real entropy, and fails if that never
happens.

The distinction the kernel now draws is between being *keyed* and being
*seeded*. `rng::init` has always mixed in HPET reads, TSC jitter and APIC tick
counts. Those key the generator — it produces a keystream immediately, which
early boot needs for stack canaries and ASLR offsets — but on a VM booted twice
from one image they **correlate across boots**. A pool built from nothing else
is not a source of key material, and handing its output to userspace under the
name `getrandom` is the bug Linux shipped in `/dev/urandom` for years.

So there are now two separate facts:

| | means | who asks |
|---|---|---|
| `rng::is_initialized()` | the generator is keyed and will produce a keystream | kernel-internal callers (canaries, ASLR) |
| `rng::is_ready()` | ≥ 256 bits of that key material were genuinely unpredictable | `SYS_GETRANDOM`, i.e. you |

Credit comes only from RDSEED/RDRAND at init, and from interrupt arrival timing
thereafter (Linux's `add_timer_randomness` third-difference test). Clock reads
are stirred in but credited nothing.

## What you have to handle: a new failure

**`SYS_GETRANDOM` can now return `KernelError::TimedOut`.** It does so when the
pool has not been credited and cannot be — the wait is bounded at 15 seconds,
and exits sooner than that as soon as the kernel can prove no interrupts are
arriving to be credited.

This is deliberate and is not a case to paper over: **do not fall back to
weaker bytes on this error.** A caller that receives an error can fail closed;
a caller handed predictable bytes cannot. `posix/src/random.rs`'s own module
doc already states the right policy — "If neither source is available we
**fail**" — so this should be a natural fit.

`posix/src/unistd.rs:2053` currently maps any kernel failure to `EIO`. `EIO` is
defensible, but if you would rather distinguish it, Linux's nearest equivalent
for "the pool is not ready and you asked to wait" does not really exist —
Linux simply blocks forever. `EIO` on a timeout is fine by me; the request here
is only that you don't silently substitute something weaker.

Under QEMU (no RDRAND, no RDSEED) the pool is credited from timer interrupts,
which at 100 Hz takes roughly a third of a second after the APIC timer starts.
Any `getrandom` from a real userspace process runs long after that, so in
practice you should never see the timeout. It is reachable only from the kernel
boot self-tests, which run before the RNG exists at all.

## The part that needs your tree: `arg2` cannot be used yet

The kernel ignores `arg2`, which is where Linux puts `GRND_NONBLOCK` /
`GRND_RANDOM` / `GRND_INSECURE`. I could not start honouring it, because
`posix/src/random.rs` reaches the kernel through

```rust
syscall2(SYS_GETRANDOM, buf as u64, len as u64)
```

and `posix/src/syscall.rs:548`'s `syscall2` declares only `in("rdi")` and
`in("rsi")`. `rdx` — where `arg2` is read from — holds whatever the compiler
last happened to leave there. If the kernel started reading it as a flags word,
every already-built binary would begin passing garbage flags, and every one of
the nine committed `services/ctest-*` ELFs is an already-built binary.

That makes it a **syscall ABI change**, which is why it is being handed to you
rather than done unilaterally. The change on your side is small:

```rust
// posix/src/random.rs
- syscall2(SYS_GETRANDOM, buf as u64, len as u64)
+ syscall3(SYS_GETRANDOM, buf as u64, len as u64, u64::from(flags))
```

Tell me when that has landed and been rebuilt into the fixtures, and I will
make the kernel read `arg2` in the same window. Until both halves are in, the
kernel must keep ignoring it — a kernel that reads flags from a caller that
doesn't set them is worse than one that ignores them.

### The gap this leaves in the meantime

`posix/src/unistd.rs:2053` **validates** the `GRND_*` flags and then discards
them. So today:

- `GRND_NONBLOCK` does not work. A caller that explicitly asked not to block
  can now be blocked (bounded, but blocked). Before this change nothing ever
  blocked, so the flag was accidentally honoured; now it is genuinely wrong.
- `GRND_INSECURE` — "give me bytes now, I accept they may be weak" — is exactly
  the escape hatch that would make the wait harmless, and it is the one flag
  that cannot be honoured without the ABI change.

I have logged this on my side as a known issue so it is not lost. It is the
main argument for doing the `syscall3` switch sooner rather than later.

## Nothing here is yours to rebuild

This touches `kernel/` only. The nine ctest fixtures do **not** need rebuilding
for this change — but note they are stale for an unrelated reason, which is
`requests/a-b-ctest-fixtures-are-stale-again-after-481da01e1.md`.

## Where to read the detail

- `kernel/src/rng.rs` — the "Entropy credit" section, `credit_entropy`,
  `add_interrupt_entropy`, `is_ready`, `wait_until_ready`.
- `kernel/src/syscall/handlers.rs` — `sys_getrandom` and `GETRANDOM_WAIT_NS`.
- `design-decisions.md` — the four tradeoffs (flags now vs later, block vs
  fail, poll vs wait queue, credit per interrupt).
