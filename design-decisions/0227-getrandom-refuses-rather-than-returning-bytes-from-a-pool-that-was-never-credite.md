## §227 — `getrandom` refuses rather than returning bytes from a pool that was never credited real entropy

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel has a random-number generator that userspace asks for
key material — the bytes behind a password, a vault salt, an encryption nonce.
It was starting up by mixing in whatever numbers were available at boot: three
different clock readings. That produces *output*, and it looks random, but on a
virtual machine booted twice from the same disk image those clocks read very
nearly the same both times — so both boots produce nearly the same "random"
bytes. Two users of the same VM image would have got the same password. The
kernel now separates "the generator is running" from "the generator was given
something genuinely unpredictable to start from", and userspace only gets bytes
once the second is true. If that never becomes true, the call returns an
**error** instead of bytes. This is the same bug, and the same fix, that Linux
went through with `/dev/urandom`.

### The problem

`rng::init` seeded a ChaCha20 generator from RDSEED/RDRAND *if the CPU had
them*, and otherwise from HPET reads, TSC jitter and APIC tick counts. Under
QEMU — which is where every boot test runs — the CPU has neither, so in
practice the pool was always the clock-only one. `SYS_GETRANDOM` returned from
it unconditionally.

Lane C's `requests/c-a-userspace-entropy-syscall.md` named the exact failure in
advance: it asked for bytes "different across two boots of an
identically-configured VM", and cited Linux's original `/dev/urandom` as the
cautionary case. The syscall as written could not promise that.

### The decision

Track two independent properties instead of one:

| | means | consulted by |
|---|---|---|
| `is_initialized()` | keyed; produces a keystream | kernel-internal callers — stack canaries, ASLR offsets |
| `is_ready()` | ≥ 256 bits of the key material were genuinely unpredictable | `SYS_GETRANDOM` |

Only RDSEED (32 bits/word), RDRAND (64 bits/call) and interrupt-arrival timing
earn credit. Clock reads still stir the pool — they cost nothing and can only
help — but are credited **zero**. `SYS_GETRANDOM` waits on `is_ready()` and
fails if it times out.

Kernel-internal callers deliberately keep using the uncredited pool. A stack
canary is wanted at boot, before any pool could possibly be credited, and a
weak canary is better than none; blocking there would deadlock the boot.

### Four tradeoffs resolved inside this, each with a real other side

**1. Block, or fail, or return weak bytes?** Block, bounded, then fail.

Returning weak bytes is what we are fixing. Blocking forever is what Linux
does, and it has hung real boots — an unbounded wait converts an entropy
shortage into an unbootable machine, with no diagnostic. The wait is therefore
capped at 15 s. *Against* the cap: a caller could in principle be refused on a
machine that would have been ready at 16 s. That is acceptable because the
failure is loud and the caller can retry, whereas the alternative failure —
predictable key material — is silent and permanent. Lane C's request explicitly
allowed "fails with a distinguishable error", so this is within what was asked
for.

**2. Wait queue, or poll?** Poll, at 25 ms.

A wait queue is the idiomatic answer and was the first design. It does not fit:
the readiness condition is published from hard-IRQ context, where the only
legal wake primitive is `WaitQueue::try_wake_one` (`wake_all` takes a blocking
spinlock and would deadlock against an interrupted lock holder), and
`WaitQueue` has no *timed* wait, which this must have. Bouncing the wake
through a work item to reach `wake_all` reintroduces "was the submission
dropped?", which then needs its own timeout fallback — a poll, one level down.
*Against* polling: it burns a task slot for up to 15 s. That cost is bounded
and only exists during boot.

**3. Honour the `GRND_*` flags now, or later?** Later, and it is lane B's move
first.

libc reaches this syscall through a two-argument stub, so the register `arg2`
is read from holds whatever the compiler last left there. Reading it as flags
would make every already-built binary pass garbage — including all nine
committed `services/ctest-*` ELFs. It is a syscall ABI change and is sequenced
with lane B (`requests/a-b-getrandom-now-waits-for-a-credited-pool.md`). The
cost of waiting is a real gap, logged in `known-issues.md`: `GRND_NONBLOCK` is
now genuinely wrong, because before this change nothing ever blocked and the
flag was accidentally honoured.

**4. How much credit per interrupt?** Linux's third-difference test, capped at
8 bits.

An interrupt is credited only when the first, second and third differences of
successive arrival times are all non-zero, and earns `ilog2(min of the three)`
bits. The point of the test is that a *perfectly periodic* source earns
nothing: an idle machine's timer tick is a metronome, and without this it could
talk the pool into declaring itself seeded on pure regularity. Linux caps at 11
bits; we cap at 8, which needs ~32 qualifying ticks — about 0.32 s at 100 Hz.
*Against* the lower cap: it is slower to become ready. In exchange it is more
conservative about how much any single observation is worth, and 0.32 s is
invisible next to the rest of boot.

That paragraph was a prediction when it was written. The 2026-08-18 boot test
confirmed it almost exactly: `32 of the first 33 interrupts qualified`, i.e.
32 × 8 = 256 bits, with the third-difference test rejecting exactly one tick —
330 ms of ticking. Worth stating precisely what that interval is measured
*from*, because the absolute number looks alarming and is not: the pool became
ready **8188 ms into boot**, but 7.86 s of that is the stretch between
`hpet::init` (`main.rs:1079`) and `cpu::sti` (`main.rs:1595`), during which no
interrupts arrive at all because none can. Entropy accrual accounts for the
last 330 ms of it. Anything that could be called userspace starts after `sti`,
so the gate is invisible to it.

### The bug this decision had, for about an hour: a guarantee with two doors

Gating the native `SYS_GETRANDOM` (90) guaranteed nothing on its own, because
there are **two** getrandom entry points. The Linux-ABI translation (318,
`kernel/src/syscall/linux.rs`) fills from the same CSPRNG, had no gate, and
documented that as "we never block". A caller who wanted uncredited bytes could
simply ask through the other door.

This was caught by the boot log rather than by reasoning — the serial output
showed a `getrandom` self-test *passing* at line 790, hundreds of lines before
`rng::init` runs at all, which is impossible for a gated path. It is worth
recording as a class of mistake rather than a one-off: **when a guarantee is
added to a syscall, the compatibility translation of that same syscall is a
second implementation and needs the same audit.** The two live in different
files, are written at different times, and nothing in the type system links
them. There are ~450 Linux syscall translations in `linux.rs`; any future
kernel-side guarantee attached to a native syscall has this same trap.

The fix also produced something better than parity. 318 receives a genuine
three-argument call, so it could implement the **full** Linux semantics that
the native path cannot yet reach: `GRND_INSECURE` skips the gate (and is now
the only way to obtain uncredited output), `GRND_NONBLOCK` returns `EAGAIN`
instead of waiting, `GRND_RANDOM` shares pool and gate as Linux has since 5.6.
So the compatibility path is temporarily *more* correct than the native one,
and the native one now has a written, tested reference to transcribe from once
lane B widens the libc stub.

Both share `GETRANDOM_WAIT_NS` rather than each defining a timeout, so they
cannot drift on what "too long" means.

### The early-boot hazard this had to survive

`main` runs the syscall self-tests (which dispatch `SYS_GETRANDOM`) at a point
where the HPET, the TSC calibration and `rng::init` itself have all not yet
run. A wait started there has no timebase to measure a deadline against and
nothing to spin on — it could not be bounded, and would have hung the boot.
`wait_until_ready` therefore refuses to sleep at all unless `is_initialized()`,
which is both the correct answer (nothing has been credited and nothing can be)
and the cheap one. The bounded-poll logic is the second line of defence, for
the real case where interrupts are running but somehow not crediting.

### Consequences

- `SYS_GETRANDOM` can now fail (`KernelError::TimedOut` → `EIO`). Callers must
  not fall back to a weaker generator on that error; doing so reinstates the
  bug.
- The boot self-test now exercises the **refusal** path, not the success path,
  because under QEMU the pool is uncredited at self-test time. It additionally
  asserts the buffer was left untouched — a handler that filled it and then
  reported an error would defeat the guarantee for any caller ignoring the
  return value.
- On hardware with RDRAND/RDSEED the pool is credited outright in `rng::init`
  and nothing ever waits.

**Revisit if** we gain a hardware RNG driver (virtio-rng would credit the pool
instantly under QEMU and make the interrupt-timing path a fallback rather than
the only source), or once the `syscall3` ABI change lands and `GRND_INSECURE`
can give callers a real escape hatch from the wait.

> **UPDATE 2026-09-15 (lane B, standing expiry audit in `todo.txt`): THE
> SECOND CONDITION HAS FIRED — and the decision survives it.**
>
> * **`GRND_INSECURE` works.** The `syscall3` ABI change landed 2026-08-18
>   (`requests/a-b-getrandom-kernel-now-reads-arg2-step-2-landed.md`), and the
>   flag is honoured and self-tested: `kernel/src/syscall/dispatch.rs` asserts
>   `getrandom(32, GRND_INSECURE)` returns 32 bytes *in the uncredited pool
>   state*, which is the escape hatch this clause was waiting for.
> * **The first condition has NOT fired.** There is still no hardware RNG
>   driver. `kernel/src/fs/hwrng.rs` looks like one and is not — it is an
>   entropy *accounting* surface (`record_generation`, `pool_status`), and
>   `VIRTIO_ID_RNG` appears only in device-ID tables. Stated because the module
>   name alone would have been enough to call this fired, wrongly.
>
> **What the revisit concludes: nothing here changes.** Waiting for a credited
> pool is still right, and `TimedOut → EIO` is still the honest failure. What
> the escape hatch changes is the *advice*, so the Consequences bullet above
> now has a companion: a caller that genuinely cannot wait should pass
> `GRND_INSECURE` and accept what that means, **not** substitute its own
> generator. The flag makes the weak draw explicit at the call site instead of
> hidden in a fallback branch.
>
> **Checked rather than assumed:** no caller in `posix/` or `userspace/` falls
> back to a weaker generator on a failed `getrandom`. That is the defect this
> entry warns about, and it is absent — a clean negative, recorded so the next
> audit does not have to re-derive it.

**Reference:** Linux `drivers/char/random.c` — `crng_init`, `crng_ready()`,
`credit_init_bits()`, `add_timer_randomness()`, and the 5.18 rewrite that
replaced the old entropy-estimator with this accounting.
