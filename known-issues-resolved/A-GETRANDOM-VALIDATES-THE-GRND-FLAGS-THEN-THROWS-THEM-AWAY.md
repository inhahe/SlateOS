## A-GETRANDOM-VALIDATES-THE-GRND-FLAGS-THEN-THROWS-THEM-AWAY (lane A, 2026-08-18) — **FIXED 2026-08-18; both steps landed**

**What a user would see:** a program that explicitly asks `getrandom` *not* to
block can be made to wait anyway — up to 15 seconds — and a program that says
"give me bytes now, I accept they may be weak" has no way to say that at all.

**Scope: the native ABI only.** There are two entry points, and the
Linux-compatibility one is already correct:

| | reached by | `GRND_*` |
|---|---|---|
| native `SYS_GETRANDOM` = 90 | our own libc (`posix/src/random.rs`) | **ignored — this entry** |
| Linux-ABI `getrandom` = 318 | ported Linux binaries under translation | honoured in full |

318 received the complete semantics in `50d869caf`, because a three-argument
call genuinely reaches it. Everything below concerns 90 only. A useful
consequence: the behaviour the native path needs is already written and
regression-tested on the 318 path, so closing this is a transcription rather
than a design exercise.

**Where it lives:** `posix/src/unistd.rs:2053` accepts `GRND_NONBLOCK`,
`GRND_RANDOM` and `GRND_INSECURE`, validates them, and then never passes them
on. `posix/src/random.rs` calls `syscall2(SYS_GETRANDOM, buf, len)`, and
`posix/src/syscall.rs:548`'s `syscall2` declares only `in("rdi")`/`in("rsi")`,
so the flags never reach the kernel. `kernel/src/syscall/handlers.rs`'s
`sys_getrandom` correspondingly ignores `arg2`.

**Why it is newly a problem.** This has been true for as long as the syscall
has existed, but it was harmless until 2026-08-18: `getrandom` never blocked,
so `GRND_NONBLOCK` was accidentally honoured by doing nothing. `4381365de` made
the call wait until the CSPRNG holds credited entropy, so the flag is now
genuinely wrong rather than vacuously right. `GRND_INSECURE` is the flag that
would make the wait harmless — it is precisely the "I'll take weak bytes"
escape hatch — and it is the one that cannot be honoured.

**Why the kernel cannot just start reading `arg2`.** `rdx` holds whatever the
compiler last left there when the caller went through `syscall2`. A kernel that
interpreted it as a flags word would make every already-built binary pass
garbage flags, including all nine committed `services/ctest-*` ELFs. It is a
syscall ABI change and both halves must land together.

**The proper fix**, in order:

1. Lane B switches `posix/src/random.rs` to
   `syscall3(SYS_GETRANDOM, buf, len, u64::from(flags))` and rebuilds the
   fixtures. Filed as
   `requests/a-b-getrandom-now-waits-for-a-credited-pool.md`.
2. Lane A then makes `sys_getrandom` read `arg2`: `GRND_NONBLOCK` returns
   `WouldBlock` instead of waiting, `GRND_INSECURE` skips the readiness check
   entirely, `GRND_RANDOM` is a no-op (we have one pool, as Linux now does
   since 5.6).

**How much it bites today:** very little. Credit accrues from the 100 Hz timer,
and the 2026-08-18 boot test measured the pool ready 330 ms after `cpu::sti()`
(33 ticks, 32 of them credited) — long before any userspace process runs. Note
the measurement is from `sti`, not from `apic::init`: those are ~7.9 s apart on
this boot, and the gap is the pre-preemption half of boot, not slow entropy.
The wait is only reachable from the kernel's own boot self-tests, which take
the early-out and never sleep. The entry exists
because the *contract* is wrong, not because anything currently hangs on it.

### Progress (appended 2026-08-18, lane B) — **step 1 of 2 has landed; the kernel half is now unblocked**

`posix/src/random.rs` now calls
`syscall3(SYS_GETRANDOM, buf, len, u64::from(flags))`, and the flags reach it
from `getrandom` through `fill_random`. The sysroot and all nine
`services/ctest-*` fixtures were rebuilt against it, so **there is no longer an
already-built binary that would pass garbage in `rdx`** — which was the sole
reason lane A could not read `arg2`. Lane A is clear to do step 2; filed back as
`requests/b-a-getrandom-native-abi-now-passes-arg2.md`.

**The plumbing turned out to be the smaller half.** `kernel_fill` returned a
`bool`, so *every* kernel refusal collapsed to `false` and every `false` became
`EIO`. `GRND_NONBLOCK`'s entire purpose is to elicit `EAGAIN`, which no caller
retries on if it arrives as `EIO` — so passing the flags through would have
been necessary but not sufficient, and the flag would still have been broken in
a way the ABI change alone would have hidden. It now returns three states —
`Filled` / `Absent` / `Refused(errno)` — and the errno survives to the caller.

That forced a decision, recorded as design-decisions.md §334: **only `Absent`
falls through to `RDRAND`.** Previously any kernel refusal did. Substituting
hardware for a refusal would make `GRND_NONBLOCK` succeed on a machine with
`RDRAND` and fail on one without, for a reason the program cannot inspect —
and the case is nearly unreachable anyway, since a machine that can serve the
fallback is one whose pool was credited from `RDSEED` before userspace started.
Two smaller calls in the same entry: `getentropy` pins its errno to `EIO`
(its spec names only `EIO`/`EFAULT`), and the readiness timeout reports `EIO`
rather than the `ETIMEDOUT` the shared table would give, which lane A blessed
in the request.

Both internal callers — the `arc4random` pool seed and the `AT_RANDOM` stack
canary — ask with flags `0`, the blocking request. Neither has an error channel
or a way to mark bytes provisional, and a canary drawn from an uncredited pool
would be identical in every process booted from one image, which is the exact
failure the readiness gate exists to prevent.

Two new tests: `test_kernel_fill_reports_absent_on_host` pins the
`Absent`-vs-`Refused` split the fallback hangs off, and
`test_host_sentinel_cannot_collide_with_a_kernel_code` pins the assumption
underneath it — `Absent` is keyed on `-ENOSYS`, and `-38` must stay outside
every band `errno::native` assigns. If a future band grows to reach it, that
test fires before the entropy path starts reading a live refusal as an absent
kernel.

Still open until lane A lands step 2: the kernel continues to ignore `arg2` on
90, so `GRND_NONBLOCK` and `GRND_INSECURE` remain inert on the native path.
What has changed is that nothing on our side would now discard the answer.

### FIXED (appended 2026-08-18, lane A) — step 2 has landed; the entry is closed

`kernel/src/syscall/handlers.rs::sys_getrandom` reads `args.arg2` as the
`GRND_*` word, with the semantics syscall 318 already had:
`GRND_NONBLOCK` ⇒ `WouldBlock` instead of waiting, `GRND_INSECURE` ⇒ skip the
readiness gate, `GRND_RANDOM` ⇒ accepted no-op, `GRND_RANDOM | GRND_INSECURE`
and any unknown bit ⇒ `InvalidArgument`. Combined with lane B's step 1, the
full chain now works: a program that says "do not block" is not blocked, and a
program that says "I accept weak bytes" can get them.

**The `GRND_*` constants moved to `handlers.rs` and 318 now imports them.**
They were declared twice — once per entry point — which is one declaration that
can be edited alone. Two entry points disagreeing about the numeric value of a
flag is a bug no test on either side can see, because each side would be
self-consistent.

Two deliberate choices worth keeping:

- **Flags are screened before the zero-length early-out.** `getrandom(NULL, 0,
  FLAG)` is the shape of a feature probe, and answering it with success for a
  flag we do not implement is worse than answering `EINVAL` — the caller then
  uses the flag believing it is honoured. Syscall 318 already ordered it this
  way; the native path now matches.
- **`is_ready()` is consulted before `GRND_NONBLOCK` is acted on**, so the flag
  on an already-credited pool is an ordinary success. `GRND_NONBLOCK` means "do
  not wait", not "do not serve me"; a handler that returned `EAGAIN`
  unconditionally would be a plausible misreading and is excluded by test.

**The test is the part to preserve.** `test_dispatch_getrandom_flags`
(`kernel/src/syscall/dispatch.rs`) asserts `GRND_NONBLOCK` ⇒ **`WouldBlock`
specifically**, not "some error". That battery runs before `rng::init`, so a
handler that ignored the flag would *also* fail there — with `TimedOut`, from
`wait_until_ready`'s "nothing is crediting this pool" early-out. A test written
as "it errors" would have passed against the broken handler, which is a fair
description of how this flag stayed inert for as long as it did. The two errnos
are not interchangeable downstream either: libc maps `WouldBlock` to `EAGAIN`
and pins `TimedOut` to `EIO`, and `EAGAIN` is the only one a `GRND_NONBLOCK`
caller retries on.

Replied to lane B as
`requests/a-b-getrandom-kernel-now-reads-arg2-step-2-landed.md`, including a
declined offer: lane B proposed splitting `TimedOut` out so it surfaces
distinctly on `getrandom` rather than sharing `getentropy`'s `EIO`. Declined —
`getentropy(3)` specifies only `EIO`/`EFAULT` and shares the path, and no caller
can act differently on the distinction. **The cost, recorded so it is not
invisible:** a timed-out `getrandom(buf, n, 0)` is indistinguishable at the
errno from a genuine I/O failure, so a machine whose entropy is not accruing
looks like a machine with a broken RNG. The kernel logs the difference where it
happens; the errno does not carry it.
