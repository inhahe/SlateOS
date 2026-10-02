## §319 — libc's four wait entry points funnel through one call, and the extensible struct's size is deliberately not exposed to C

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — the kernel ABI was lane A's call (§206);
these are the libc-side shape questions it left open.

**In short:** a program that starts a child process can wait for it to finish
four different ways, all meaning roughly the same thing. Until now each of the
four was written out separately in our C library, and they had drifted: one of
them refused a perfectly legal request outright, one silently destroyed the
information it was asked only to look at, and two of them reported the child's
CPU time as zero when the kernel knew the real number. This change routes all
four through a single piece of code and has them read the kernel's new answer
structure. The three choices worth recording are: one shared body rather than
four; naming *what kind* of thing is being waited on with a small enum instead
of overloading the sign of a number; and **not** giving C programs a way to
name the answer structure's size, even though that is exactly what one test
needs.

### The context

Lane A's §206 gave `SYS_PROCESS_WAIT_STATUS` three new option bits (`WPGID`,
`WINFO`, `WNOWAIT`) and a `WaitInfo` out-parameter carrying the facts that do
not survive the reap — the child's uid, its CPU time, and four other counters.
`requests/a-b-wait-syscall-grew-wpgid-wnowait-and-a-waitinfo-struct.md` asked
lane B to consume it. Nothing in libc broke by not consuming it; every choice
below is about the shape of the consumption, not whether to do it.

### The decisions

**1. One `wait_common`, not four call sites.**

`waitpid`, `wait3`, `wait4` and `waitid` all reach the kernel through one
private function. `wait3` is now literally `wait4(-1, …)`, and `wait4` is
`waitpid` plus an out-parameter.

*For:* the four differ only in how they *encode* an answer — a packed `wstatus`
word, a `siginfo_t`, a `struct rusage` — never in which child they select or
whether a report is consumed. Those were the parts that drifted. It is the same
argument §206 makes one level down, and it is why `waitid` inherited the
group-1 `ENOSYS` and the ignored `WNOWAIT` in the first place: it was a
separate copy that nobody updated when `waitpid` grew.

*Against:* a funnel makes each caller pay for the union of the others'
concerns — `waitpid`'s hot path now goes through a `match` on a two-armed enum.
Accepted: the match compiles to a branch on a register, next to a syscall that
costs three orders of magnitude more, and the alternative is the drift that
produced this request.

**2. `WaitTarget { Selector, Pgid }` at the libc boundary, mirroring the
kernel's `WaitTarget { Any, Pid, Pgid }`.**

*For:* the whole of request item 1 is that `waitpid`'s single signed integer
cannot express "group 1" — it spends `-1` on "any child". A libc that kept
passing a signed integer around internally and only converted at the syscall
would have the same hole one layer up. Making the two interpretations separate
*types* is what stops a half-conversion, which is the specific failure mode
lane A warns about: with `WPGID` set, `-g` is not group `g`, it is a huge
unsigned pgid that matches nothing and silently returns `ECHILD`.

*Against:* two enums, one per side of the syscall, describing the same idea. A
single shared definition would be less to keep in step. Rejected because libc
and the kernel are separately compiled and separately versioned — a shared type
would be a shared *header*, which is precisely the coupling the numbered ABI
exists to avoid.

Note the deliberate asymmetry: libc's enum has two arms where the kernel's has
three. `Any` is not a libc concept — `waitpid(-1, …)` and `waitid(P_ALL, …)`
both mean it, and both are already spelled as a selector. Adding a third arm
would create two encodings of the same wait inside libc, which is the disease.

**3. The `WaitInfo` size is not reachable from C.**

The extensible-struct convention only means anything if a caller can pass a
size other than the current one. libc, by construction, always passes its own
`sizeof` — so libc cannot test the convention, and there is a real temptation
to export a size-taking entry point (`slate_wait_info(…, size)`) so that a
fixture can.

*Rejected.* A libc export whose only correct caller is a test is a permanent
hole: every other caller can then hand the kernel an arbitrary length, and the
kernel's validation is all that stands between a typo and a write past the end
of a buffer. The convention's real beneficiary is *an older libc on a newer
kernel*, which does not need an entry point — it needs the kernel to honour a
size it already passes.

So the test reaches the syscall raw, from C, in
`services/ctest-jobctl/main.c` (checks 150-187): a hand-written five-argument
`syscall`, a locally-declared 72-byte struct, and a 128-byte buffer poisoned
with `0x5A` before each call. Check 157 asserts that `arg4 = 128` comes back
with bytes 72..128 **zeroed**; check 169 asserts that `arg4 = 24` leaves bytes
24..128 **exactly as poisoned**. Both are properties of `copy_to_user`, which no
kernel self-test can reach — a bare kernel task has no user address space, so
`dispatch.rs::test_dispatch_wait_info_layout` can only check the pure encoder.

The struct is re-declared in the fixture rather than shared with the kernel's
definition, and that is the point: a struct generated from the same source as
the kernel's would agree with it by construction even if both were wrong. The
fixture is asserting what bytes arrive, not what the kernel meant to send.

**4. The unit split is honoured in exactly one function.**

`WaitInfo` is in microseconds; `siginfo_t`'s `si_utime`/`si_stime` are in
USER_HZ ticks, because the field is `clock_t` and a ported binary reading it
expects what Linux's `nsec_to_clock_t` would have put there. libc converts in
`us_to_clock_t` and nowhere else.

*For:* a caller that guessed the wrong unit is off by exactly 10× — large
enough to be wrong, small enough to look plausible in a log, and invisible in
review. One conversion site is one place to be wrong.

*Against:* it means `wait4`'s `rusage` (microseconds, via `us_to_timeval`) and
`waitid`'s `siginfo` (ticks) report the same child's CPU time at different
resolutions, and a program that used both would see them disagree below 10 ms.
Accepted: that disagreement is Linux's, and matching it is the entire reason
the ABI has two units.

**5. `wait4` writes `rusage` on a `WNOHANG` miss but not on an error.**

*For:* both match Linux's `sys_wait4`. On a miss there was no child, so zero
usage is the truth and leaving the caller's previous contents would read as a
second child's numbers. On an error there is nothing to describe.

The old code did neither — it zero-filled unconditionally, before it even knew
whether a child existed, via the kernel's `clear_user_rusage`. That was a
**false zero**: `pcb`'s counters existed the whole time and the call was
discarding them. Two host tests asserted the zeroing and had to be inverted;
they now assert the Linux contract instead.

**Where it lives.** `posix/src/process.rs` — `WaitInfo`, `WaitTarget`,
`wait_common`, `waitid_target`, `waitid_kernel_options`, `siginfo_for_wait`,
`rusage_from_wait_info`, `us_to_clock_t`, `us_to_timeval`; `posix/src/resource.rs`
(`Rusage` gains `Default`, so a partial fill has an honest base);
`services/ctest-jobctl/main.c` checks 150-187.

**Tests.** Host: 18 new tests over the pure helpers — the `WaitInfo` byte layout
(every offset, both zero pads, size 72, align 8), the `(idtype, id)` →
`WaitTarget` mapping including group 1 and the never-negate rule, the option
translation, and the `rusage`/`siginfo` encoders. Target: `ctest-jobctl`
150-187, which is the only layer that can see a `copy_to_user`.
