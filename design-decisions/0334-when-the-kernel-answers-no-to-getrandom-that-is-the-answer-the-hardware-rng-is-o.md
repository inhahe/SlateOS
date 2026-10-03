## §334 — When the kernel answers "no" to `getrandom`, that is the answer; the hardware RNG is only for when there is no kernel

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** Our C library can get random bytes from two places: the kernel,
or a CPU instruction (`RDRAND`) that generates them directly. Until now, if the
kernel refused for any reason, we quietly tried the CPU instead. The kernel has
just learned to refuse in a *meaningful* way — "I don't have enough real
randomness yet, and you told me not to wait" — and quietly overriding that
refusal would throw the message away. From now on the CPU is used only when
there is no kernel at all to ask (which in practice means our test builds on
Windows). If a kernel is present and says no, we report its answer to the
caller unchanged.

### What forced the choice

Lane A's kernel now blocks `SYS_GETRANDOM` until its random pool has been
*credited* with genuinely unpredictable input, and it honours the `GRND_*`
flags — in particular `GRND_NONBLOCK`, which means "fail rather than make me
wait" and which must report `EAGAIN` (the standard "try again later" error).
Our side threw that away twice over:

1. `posix/src/random.rs` reached the kernel through `syscall2`, which declares
   only two argument registers, so the flags word never arrived. That half is
   not a decision — it is just a fix (`syscall3`).
2. `kernel_fill` returned a **`bool`**. Every refusal, whatever it meant,
   became `false`, and `false` became `EIO`. `EIO` is not a value any caller
   retries on, so `GRND_NONBLOCK` could not have worked even with the flags
   plumbed through.

Fixing (2) means carrying the kernel's errno out. Once it is carried out, the
question below becomes unavoidable — the old code answered it by accident.

### The decision

`kernel_fill` now returns three states rather than two: `Filled`, `Absent`
(there is no kernel — the host build's `-ENOSYS`), and `Refused(errno)`. Only
`Absent` falls through to `RDRAND`/`RDSEED`.

| | Old | New |
|---|---|---|
| Kernel filled the buffer | success | success |
| No kernel to ask | try `RDRAND` | try `RDRAND` |
| Kernel present, declined | **try `RDRAND`** | **report its errno** |

*What changes observably:* on a machine with `RDRAND`, a `getrandom(...,
GRND_NONBLOCK)` issued before the kernel pool is credited used to return good
bytes; it now returns `-1`/`EAGAIN`.

### Why, and the case against

The case *for* the old behaviour is real and should be stated plainly:
`RDRAND` is a genuine hardware CSPRNG, not a weak source. Substituting it
yields bytes that are perfectly good as key material. Refusing where we could
have succeeded is, in isolation, a worse outcome for the caller.

Three things outweigh it:

1. **A silent override makes the refusal unobservable, and inconsistently so.**
   The same program with the same flags would take different branches on two
   machines for a reason it cannot inspect — `EAGAIN` where there is no
   `RDRAND`, success where there is. A caller using `GRND_NONBLOCK` is by
   definition one that has a fallback plan; denying it the signal it asked for
   is worse than either answer given consistently.
2. **The case is nearly unreachable, so the cost is close to zero.** The kernel
   credits its pool from `RDSEED`/`RDRAND` at init. A machine that could serve
   the fallback is therefore a machine whose pool was credited before userspace
   started, so "kernel declined for want of entropy" and "hardware RNG
   available" are very nearly mutually exclusive. Under QEMU (no `RDRAND`) the
   fallback could not have helped anyway.
3. **It makes "the kernel is broken" visible.** A kernel that does not
   implement syscall 90 now surfaces as an error instead of being papered over
   by a userspace substitute — the same reasoning as §462 below, and as this
   module's standing refusal to invent bytes from a clock.

The narrow scope is what keeps this safe: `Absent` is keyed on `-ENOSYS`, which
every `syscallN` returns on the host build, and `-38` lies in none of the bands
`errno::native` assigns (`-1..=-9`, `-100`, `-200`, `-300`, `-400`, `-500`,
`-600`), so no real kernel code can collide with it. A test pins that
(`test_host_sentinel_cannot_collide_with_a_kernel_code`): if a future band ever
grows to reach `-38`, it fires before the entropy path starts reading a live
refusal as an absent kernel.

### Three smaller calls inside this one

- **`getentropy` pins its errno to `EIO`** rather than passing the kernel's
  through. `getentropy(3)` specifies exactly two failures, `EIO` and `EFAULT`,
  and a caller written to that spec would not recognise a third. `getrandom`,
  which has no such constraint, passes the kernel's value through.
- **The readiness timeout reports `EIO`, not `ETIMEDOUT`.** The kernel's
  `TimedOut` maps to `ETIMEDOUT` in the shared table, which is honest but which
  no portable caller tests for; `getentropy`'s specified "could not fill the
  buffer" value is `EIO`, and both calls share this path. Every other code
  passes through untouched, which is exactly what keeps `WOULD_BLOCK` →
  `EAGAIN` intact. Lane A explicitly blessed `EIO` here.
- **Both internal callers ask with flags `0`** — the blocking request. The
  `arc4random` pool seed and the `AT_RANDOM` stack canary have no error channel
  and no way to mark bytes provisional, so they must *wait* for a credited pool
  rather than accept whatever is available. A canary drawn from an uncredited
  pool would be identical in every process booted from one image, which is the
  exact failure the kernel's readiness gate exists to prevent.
