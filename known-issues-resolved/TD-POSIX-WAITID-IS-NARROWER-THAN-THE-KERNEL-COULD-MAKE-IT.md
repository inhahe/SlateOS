### TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT. `waitid` cannot express process group 1, cannot honour `WNOWAIT`, and reports `si_uid` as 0 — all three because `SYS_PROCESS_WAIT_STATUS` is `waitpid`-shaped — LOGGED 2026-08-16 by lane B — ✅ FIXED 2026-08-16 (kernel by lane A, libc by lane B)

**Status: closed on both sides** (2026-08-16). All three gaps — and the
`si_utime`/`si_stime`/`rusage` fourth one below, whose premise turned out to be
false — are expressible through the syscall (lane A) *and* used by libc (lane
B). Stays here rather than moving to `known-issues-resolved.md` until the fix
has been on `main` through a full boot test.

**What libc now does** (`posix/src/process.rs`): `waitpid`, `wait3`, `wait4` and
`waitid` all funnel through one `wait_common`, which takes a
`WaitTarget { Selector, Pgid }` rather than a signed integer — the same shape
the kernel resolved to in §206, so the two cannot disagree about which child a
selector names. `waitid(P_PGID, 1, …)` returns the kernel's `ECHILD`/success
instead of `ENOSYS`; `WNOWAIT` maps to the kernel bit instead of being dropped;
`si_uid`, `si_utime` and `si_stime` come from the `WaitInfo` out-parameter; and
`wait3`/`wait4` write a genuine `rusage` from the same struct.

**The `WINFO` gate is respected.** `wait_common` only sets it — and only then
issues a five-argument syscall — when a `WaitInfo` was actually asked for. The
three-argument path still goes through `syscall3`, which never writes `r10`/`r8`,
so the kernel never reads registers this libc did not set. Lane A's first
version read them unconditionally and broke `ctest-pgroup`, `ctest-jobctl` and
`ctest-ctty` within one boot.

**Units.** `WaitInfo` is microseconds and `siginfo_t.si_utime`/`si_stime` are
USER_HZ ticks, deliberately. libc converts in exactly one place
(`us_to_clock_t`); a second conversion site would be off by exactly 10×, which
is the kind of error that survives review.

Rationale: `design-decisions.md` §206 (kernel) and §319 (libc).

**Correction to item 4 below ("there is no per-process CPU accounting").** That
was already false when this entry was written. `pcb`'s `acct_*`/`child_*`
counters and `thread::process_cpu_ticks`/`process_fault_counts`/
`process_ctxsw_counts` were live, and `sys_getrusage` already encoded all 144
bytes from them; `wait4`'s `clear_user_rusage` was a **false zero** — the data
existed and the call threw it away. `wait4` now writes a real `rusage` and
`waitid` fills `si_utime`/`si_stime` in USER_HZ ticks.

**In short:** `waitid()` is the modern replacement for `waitpid()`, and it exists
because `waitpid()`'s arguments cannot express some perfectly reasonable waits.
libc's `waitid` is now real rather than a stub — it fills the caller's
`siginfo_t` and supports process-group waits, both of which it previously did
not — but it is built on the same kernel call `waitpid` uses, so it inherits
exactly the limits `waitid` was invented to escape. Three remain. None breaks
anything we ship; each is now *reported* honestly instead of faked.

**Where it lives.** `posix/src/process.rs::waitid` and `siginfo_for_wstatus`;
the kernel side is `kernel/src/syscall/handlers.rs::sys_process_wait_status`
(~3993) and `wait_pgid_filter` (~3771).

| Gap | libc's behaviour today | Why libc cannot fix it alone |
|---|---|---|
| `waitid(P_PGID, 1, …)` from a caller not in group 1 | `ENOSYS` | `waitpid`'s selector uses `-1` for "any child", so group 1 has no encoding. The kernel infers the group filter from that one signed integer. |
| `WNOWAIT` (observe a child without reaping it) | accepted, **not honoured** — the child is reaped anyway | The syscall's option mask is `WNOHANG\|WUNTRACED\|WCONTINUED` and the wait path reaps unconditionally. There is no peek mode to ask for. |
| `siginfo_t.si_uid` (the child's real uid) | always `0` | The child is reaped by the time the call returns; `SYS_PROCESS_GET_CREDENTIALS` takes no pid and reports only the caller's own. |

Also zero for the same "nothing can source it" reason: `si_utime`/`si_stime` —
there is no per-process CPU accounting, which is why `wait3`/`wait4` have always
zeroed their `rusage` too.

**Why `si_uid` is 0 rather than the caller's uid.** Substituting `getuid()`
would be correct for a child that never changed credentials and wrong for
precisely the case in which a caller would bother to read the field — a child
that dropped privilege. That is the same rule design-decisions.md **§314** sets
for capabilities: libc must not invent an answer it does not have. A zero is
visibly unsourced; a plausible wrong uid is not.

**Why the group-1 case is `ENOSYS` rather than a silent `waitpid(-1, …)`.** A
wait for any child *reaps* some child, consuming its status permanently. A
caller that asked for a group wait and got an arbitrary child has no way to
detect the substitution and has lost the status of a child it was not managing.
Refusing is recoverable; guessing is not.

**Proper fix — all three were kernel-side, and all three landed** on
2026-08-16 (lane A), exactly as this entry proposed:

1. ✅ **`WPGID = 1 << 16`** on `SYS_PROCESS_WAIT_STATUS` makes `arg0` an
   unsigned pgid, so group 1 is nameable. Previously rejected by the mask, so
   no existing caller is affected.
2. ✅ **`WNOWAIT = 1 << 24`** peeks: the zombie is left in place and the
   job-control report is left unconsumed, so an identical second wait sees the
   same event again.
3. ✅ **The child's uid** rides out in a new `WaitInfo` out-parameter (`arg3`
   pointer, `arg4` size, 72 bytes), which — as this entry predicted — also
   carries the CPU times and four other counters. It is written under the
   `clone3`/`sched_setattr` convention (`min(caller, kernel)` bytes, zero-filled
   tail), so it can grow again without a new syscall number.

One thing nobody asked for came out of the same work: **`sys_waitid` had no
signal handling at all** — no pending-signal check, no `ERESTARTSYS`, no
signalfd registration — so a process blocked in it could not be interrupted.
`wait4` next door had all three. Unifying the three wait syscalls onto
`kernel/src/syscall/wait.rs` fixed it, and it is the clearest evidence for why
one primitive beats three copies.

`waitid`'s doc comment now carries the closed list under "Three limitations that
are gone as of 2026-08-16", which is where someone debugging a `WNOWAIT` that
reaped will actually look, plus the two divergences from Linux that genuinely
remain (`P_PIDFD` is `EINVAL`, and `si_utime`/`si_stime` have tick resolution
even though the kernel knows microseconds — the field is `clock_t` and a ported
binary expects ticks).

**Coverage.** On target, `services/ctest-jobctl/main.c` checks 100-111
(`CLD_STOPPED`/`CLD_CONTINUED` against a real kernel-encoded job-control event,
plus the proof that observing one consumes it), 120-132 (`CLD_EXITED` with the
exit *code* in `si_status`, `ECHILD` on re-wait, `CLD_KILLED`/`SIGKILL`),
140-147 (argument validation) and — new with the fix — **150-187**: the
`WaitInfo` fill including `uid` and the six counters, `WNOWAIT` peeking twice
without reaping, and naming a process group including group 1. The fixture is
the only place the `wstatus` → `siginfo_t` decoding is testable at all:
`waitpid`'s syscall arm is compiled out for anything but `target_os = "none"`,
so a host `waitid` returns `ENOSYS` before a status word exists to decode.

Checks **157** and **169** are the two lane A asked lane B for by name, and they
are the only tests in the tree that can exist anywhere else: they pass sizes
libc by construction never passes (128 and 24 against a 72-byte struct) to prove
the kernel zero-fills the tail and never writes past the caller's declared
length. A kernel self-test task has no user address space, so it reaches the
pure encoder but not `copy_to_user`, and truncation is a property of the copy.
They reach the syscall raw rather than through a size-taking libc export,
because such an export would be a permanent hole letting every other caller hand
the kernel an arbitrary length for the sake of one test.
