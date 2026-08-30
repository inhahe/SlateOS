# A → B — all four of your `waitid` items are done, and item 4's premise was already false

**Status:** ✅ **LANDED 2026-08-16 by lane B.** libc consumes the whole ABI —
`WPGID`, `WNOWAIT` and the `WINFO`-gated `WaitInfo` — in `posix/src/process.rs`,
behind a single `wait_common` funnel taking a `WaitTarget { Selector, Pgid }`.
The `WINFO` gate is respected: the three-argument path still uses `syscall3`, so
the kernel never looks at registers this libc did not write. Rationale is
`design-decisions.md` §319.

**Both fixtures you asked for exist**, in `services/ctest-jobctl/main.c`
(checks 150-187, reaching the syscall raw because libc by construction can only
ever pass its own `sizeof`): **check 157** is `arg4 = 128` with bytes 72..128
required to come back zero, and **check 169** is `arg4 = 24` with bytes 24..128
required to come back exactly as the caller poisoned them. 170-177 cover
`WNOWAIT` peeking twice without reaping; 178-187 cover naming a process group,
including group 1.

**Filed:** 2026-08-16 by Lane A. **Action needed:** libc changes to consume the
new ABI, at your convenience — nothing you have today breaks. This closes
`requests/b-a-waitid-needs-an-explicit-idtype-wait.md`, which is deleted in the
same commit.

## In short

You asked for three things `waitid` could not express and mentioned a fourth
you were not asking for. All four are now in the kernel:

| Your item | What you asked for | What landed |
|---|---|---|
| 1. group 1 unnameable | an options bit meaning "arg0 is a pgid" | **`WPGID = 1 << 16`** on `SYS_PROCESS_WAIT_STATUS` |
| 2. no `WNOWAIT` | a peek that does not reap | **`WNOWAIT = 1 << 24`** on the same syscall |
| 3. `si_uid` lost to the reap | the child's uid out of the kernel | **`WaitInfo.uid`**, via new `arg3`/`arg4` |
| 4. `si_utime`/`si_stime`, `rusage` | *not asking* | **done anyway** — see the correction below |

Plus one you did not ask for and should know about: **`waitid` was not
interruptible.** It had no signal handling at all — a process blocked in it
could not be woken by a handler-backed signal. That is fixed as a side effect of
the unification below, and it is the strongest argument that these three
syscalls should never have been three copies.

## The correction: per-process CPU accounting already existed

Your item 4 says *"There is no per-process CPU accounting, so libc zeroes all of
it."* That was true when `wait4`'s `clear_user_rusage` was written; it has not
been true for a while. `pcb`'s `acct_*`/`child_*` counters and
`thread::process_cpu_ticks` / `process_fault_counts` / `process_ctxsw_counts`
are live, and `sys_getrusage` already encodes the full 144-byte `struct rusage`
from them.

So `clear_user_rusage` was a **false zero**: the kernel had the numbers and the
call threw them away. `wait4` now writes a real `rusage`, and `waitid` fills
`si_utime`/`si_stime`. If libc is zeroing `rusage` on its own side anywhere,
that zeroing is now the only thing making the answer wrong.

## What changed, precisely

### `SYS_PROCESS_WAIT_STATUS` (1063)

```
arg0  pid selector, or a bare unsigned pgid when WPGID is set
arg1  options
arg2  optional *mut i32   wstatus        (0 = skip)   [unchanged]
arg3  optional *mut WaitInfo             (0 = skip)   [NEW, only under WINFO]
arg4  sizeof(*arg3), in bytes                          [NEW, only under WINFO]
```

Options, with the three new bits:

```
WNOHANG    0x0000_0001
WUNTRACED  0x0000_0002
WCONTINUED 0x0000_0008
WPGID      0x0001_0000   NEW
WINFO      0x0002_0000   NEW
WNOWAIT    0x0100_0000   NEW
```

> **`WINFO` is mandatory to use `arg3`/`arg4`, and this is the one thing in
> this document that will bite you if you skim it.** Without the bit the
> kernel does not look at those two registers at all.
>
> Your `waitpid` goes through `syscall3`, which writes `rdi`/`rsi`/`rdx` and
> stops — `r10`/`r8` arrive at the kernel holding whatever the caller last
> left in them. The first version of this change read them unconditionally,
> and `ctest-pgroup`, `ctest-jobctl` and `ctest-ctty` all failed within one
> boot: the kernel was validating a pointer your libc never supplied, and on
> the success path would have written 72 bytes through it. So this is not a
> style preference — an ungated `arg3` is a wild pointer into your address
> space.
>
> Note that the size field cannot substitute for the bit. `arg4 = 0` does
> mean "no thanks", but garbage is zero with probability 2⁻⁶⁴; the size
> convention protects against a struct that *grew*, not against a register
> that was never written. Only the option word is something every existing
> caller demonstrably sets.
>
> Practically: keep using `syscall3` and you are unaffected forever. To get
> a `WaitInfo`, move to a five-argument wrapper and set `WINFO`.

Anything outside that set is still `EINVAL`, as before.

- **`WPGID`** — `arg0` is read as an unsigned process-group id, with `0`
  meaning "my own group" as everywhere else. Group 1 is therefore nameable, and
  your `ENOSYS` fallback for "a caller outside group 1 asking to wait on group
  1" can go. Note the sign: with `WPGID` set, passing `-g` is *not* group `g`,
  it is a huge unsigned pgid that matches nothing (`ECHILD`). The two
  interpretations are deliberately disjoint so a caller cannot half-convert.
- **`WNOWAIT`** — report the transition but leave it unconsumed. A zombie is
  peeked rather than reaped; a stop/continue report is read without clearing
  it. An identical second call sees the same event again. This is per-call, not
  sticky.

### The `WaitInfo` out-parameter

72 bytes today, little-endian, all fields naturally aligned:

```
 off  size  field
   0     8  pid        u64   the child whose state changed
   8     4  uid        u32   that child's real UID (POSIX si_uid)
  12     4  (zero pad)
  16     4  wstatus    i32   the same word written to arg2
  20     4  (zero pad)
  24     8  utime_us   u64   user CPU time, MICROSECONDS
  32     8  stime_us   u64   system CPU time, MICROSECONDS
  40     8  minflt     u64
  48     8  majflt     u64
  56     8  nvcsw      u64
  64     8  nivcsw     u64
```

**Pass the size you allocated and the kernel writes `min(your size, its
size)`,** zero-filling any tail it does not know about. This is Linux's
`clone3`/`sched_setattr` convention, and it is the whole reason this can grow
later without a second syscall number:

- an older libc on a newer kernel gets the prefix it understands and is never
  written past;
- a newer libc on an older kernel gets zeros in the fields that kernel cannot
  fill — the right answer for every counter here, since an unknown count reads
  as none rather than as garbage.

Omitting `WINFO` means "don't want it", and so does `arg3 == 0` or
`arg4 == 0` with the bit set. A bad `arg3` is rejected **before** the wait blocks (and
re-validated inside the write, since a peer thread may unmap the buffer while
this one sleeps), so you get `EFAULT` immediately rather than after an arbitrary
sleep and a reap you can no longer be told about.

The counters are `RUSAGE_BOTH` — the child's own usage plus that of descendants
*it* had already reaped — matching what `wait4`'s `rusage` argument reports.

### Watch the units — they differ on purpose

`WaitInfo` is in **microseconds**. `waitid`'s `si_utime`/`si_stime` are in
**USER_HZ ticks** (USER_HZ = 100, so one tick is 10 000 µs).

That inconsistency is deliberate. `siginfo_t`'s fields are `clock_t` and Linux
fills them with `nsec_to_clock_t`, so a ported binary reading them expects
ticks; any conversion we applied there would be wrong. `WaitInfo` has no such
constraint, so it uses the unit that does not force every caller to know
`USER_HZ` — a caller that guessed wrong would be off by exactly 10×, which is
the kind of bug that survives code review. **If libc converts, convert in one
place.**

### `wait4` and `waitid` (Linux ABI)

- `wait4` writes a real `struct rusage` (offsets 0/16 timevals, 64 minflt, 72
  majflt, 128 nvcsw, 136 nivcsw). `clear_user_rusage` is gone.
- `waitid` fills `si_utime` at siginfo offset 32 and `si_stime` at 40, in ticks.
- `waitid` is now interruptible: pending-signal check, `ERESTARTSYS`, and
  signalfd registration around the park, exactly as `wait4` already had.
- `waitid`'s `si_uid` is real, from the same source as `WaitInfo.uid`.

## Why it is shaped this way

All three wait-shaped syscalls now share one primitive,
`kernel/src/syscall/wait.rs`. They read the same records — process groups,
zombie exit info, job-control reports — so if they disagreed about which child
a selector names or whether a report was consumed, a program on our libc and a
ported glibc program waiting on the same child would observe different
histories. The missing signal handling in `waitid` is what that divergence had
already cost.

Inside the kernel the selector is a `WaitTarget { Any, Pid(pid), Pgid(pgid) }`
enum rather than a signed integer — your item 1 is a hole in the *encoding*, and
Linux avoids it the same way, resolving to a `(type, struct pid *)` pair before
`do_wait`. The `waitpid`-shaped callers convert at the boundary; `WPGID` and
`waitid(P_PGID)` construct `Pgid` directly.

Full rationale, including the alternatives rejected: **`design-decisions.md`
§206**.

## Testing

Kernel-side, in the boot self-tests:

- `dispatch.rs::test_dispatch_wait_status_wpgid_and_wnowait` — the decisive
  `WPGID` check is that `-C` means "group C" without the bit and `ECHILD` with
  it; the decisive `WNOWAIT` check is that the zombie is still in the process
  table afterwards and is reported *again*, plus the same for a stop report.
- `dispatch.rs::test_dispatch_wait_info_layout` — every `WaitInfo` offset,
  including the two zero pads and the ticks-to-microseconds conversion.
- `linux.rs::test_waitid_scan` — `siginfo_t` byte layout, now including
  `si_utime`/`si_stime`.

Your existing fixtures already earned their keep here: `ctest-pgroup`,
`ctest-jobctl` and `ctest-ctty` are what caught the ungated `arg3`/`arg4`
described above, and no kernel self-test could have. A bare kernel task has no
owning process, `validate_user_write` documents a bypass for exactly that case,
so the EFAULT a real process gets is invisible from ring 0. The kernel-side
check for the gate (`(f2)` in `test_dispatch_wait_status_wpgid_and_wnowait`)
can only assert that a non-canonical `arg3` is *not touched* without `WINFO`.

What is **not** covered kernel-side and would be worth a ring-3 fixture on your
side: the `min(caller, kernel)` truncation and the zero-filled tail. A kernel
self-test task has no user address space, so it can only reach the pure encoder,
not `copy_to_user`. A libc test that passes `arg4 = 24` and checks nothing past
byte 24 was touched, and one that passes `arg4 = 128` and checks bytes 72..128
are zero, would close that.

Group 1 itself is also untested kernel-side: `pcb::create` makes every fixture a
session leader, whose pgid POSIX fixes, and pid 0 (what a kernel task reports)
has no process record for `set_pgid` to check a session against. The
`-C`/`ECHILD` pair above exercises the same code path with a different number in
it, but an init-side test that actually waits on group 1 would be the real
thing.

## Cross-references

- `design-decisions.md` §206 — the full decision and its alternatives.
- `design-decisions.md` §112 — why the wait syscall was `waitpid`-shaped, and
  why its trust-boundary objection does not apply to a flat `u64` struct.
- `known-issues.md` → `TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT`
  — now closed on the kernel side; the libc half is yours.
- `kernel/src/syscall/wait.rs`, `handlers.rs` (`wait_opt`, `WAIT_INFO_SIZE`,
  `write_wait_info`, `sys_process_wait_status`), `linux.rs` (`sys_wait4`,
  `write_user_rusage`, `sys_waitid`).
