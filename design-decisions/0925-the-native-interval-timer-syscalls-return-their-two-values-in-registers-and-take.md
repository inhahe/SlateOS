## §925 — The native interval-timer syscalls return their two values in registers, and take no pointer at all

**Date:** 2026-09-09. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** a program that asks for "wake me in five seconds" currently gets
told "certainly" and is never woken. The machinery to wake it has existed for
some time in the kernel; what was missing was a way for our own C library to
reach it. This adds that door. The design choice recorded here is a small one
with a disproportionate payoff: because both calls hand back exactly two
numbers, they return those numbers **in registers** rather than writing them
into a buffer the caller supplies — which removes an entire class of failure
from the pair before it exists.

**The request.** Lane B's
`requests/b-a-expose-the-interval-timer-natively-so-alarm-can-fire.md`:
`alarm(3)` and `setitimer(2)` in our libc validate their arguments, report
success, and arm nothing. `kernel/src/proc/itimer.rs` already backs
`setitimer(ITIMER_REAL)`, `getitimer(ITIMER_REAL)` and legacy `alarm()` with a
real timer and a real `SIGALRM` — but it is reachable only from
`kernel/src/syscall/linux.rs`, and `posix/` has no number to call. This is the
third request of this exact shape, after `SYS_PROCESS_SETGROUPS` (1067) and
`SYS_PROCESS_CHROOT` (1068): the work exists and only the door is missing.

### The decision

```
SYS_ITIMER_SET (1069)   (which, value_ns, interval_ns) -> ok2(prev_value_ns, prev_interval_ns)
SYS_ITIMER_GET (1070)   (which)                        -> ok2(value_ns, interval_ns)
```

Three choices in that, each with a real alternative.

**1. Nanoseconds, not `struct itimerval`.** Linux passes a pair of
`{tv_sec, tv_usec}` structures by pointer. Our native ABI does not have to,
and should not: `itimer.rs` already stores nanoseconds, `linux.rs` already owns
the conversion for the Linux ABI, and duplicating a 32-byte four-field layout
into the native path would mean two places that can disagree about a struct
neither of them needs. libc converts, once, where it already converts
everything else.

*Alternative rejected:* mirror `itimerval`. It buys familiarity for whoever
writes the libc shim and costs a marshalling routine, a size constant, and a
second definition of a layout to keep in step.

**2. Two return values in registers (`SyscallResult::ok2`), not an
out-pointer.** Both calls hand back exactly two `u64`s, and the syscall ABI can
already deliver a second value in `rdx`. So the pair needs **no user pointer**,
and therefore has **no `PageFault` case, no `validate_user_write`, and no
partially-applied state to reason about**.

That last one is the real prize, and it is worth spelling out because Linux
demonstrates the cost of the other choice. `do_setitimer` arms the new timer
*before* copying the old value out, so a caller that passes a bad `old_value`
pointer gets `EFAULT` **with the timer already armed** — a call that reports
failure and changed the world anyway. Our `linux.rs` arm faithfully reproduces
that, correctly, because it is emulating Linux. The native pair does not have
to inherit it, and does not: there is no copy-out step to fail.

*Alternative rejected:* `(which, value_ns, interval_ns, old_ptr)` with a
nullable pointer. Familiar, and strictly worse — it reintroduces the fault
path, the null-vs-not branch, and the arm-then-fail window, to convey two
integers the ABI can already return.

**3. `ITIMER_REAL` only; `VIRTUAL` and `PROF` are `InvalidArgument`.**
`ITIMER_REAL` counts wall-clock time and is what `alarm` and every timeout
built on it needs. The other two count CPU time consumed by the process, which
this kernel does not yet account for per-process. Refusing them is honest;
implementing them against a clock we do not keep would be the "reports success
and arms nothing" failure this whole change exists to remove — one layer down.

Lane B's request explicitly agrees: *"`ITIMER_REAL` alone unblocks every caller
below. If they are cheap, take them; if they are not, `EINVAL` on the other two
is a fine place to start."* They are not cheap, so they are `EINVAL`.

### Why this matters beyond `alarm`

**A timeout that never fires is worse than one that fails.** Lane B's audit
makes the point precisely: `signal.alarm(5)` returning success and never
firing converts a bounded wait into an unbounded one. That is not a missing
feature, it is a hang generator — and this project has now paid for it twice in
one week. `services/ctest-pty/main.c`'s header says so outright, and the
consequence is in its code:

> *"Deliberately no timeouts. `alarm`/`setitimer` report success and arm
> nothing (`known-issues.md` → `B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING`), so a
> fixture that trusted them would hang the boot test rather than fail it."*

So that fixture waits in a **counted spin** instead — and on 2026-09-09 that
spin expired before its parent could act, which is the whole of why the ^C
signal-delivery test failed with exit 44 rather than passing. A working
`alarm` would let it wait on an event with a real deadline instead of guessing
an iteration count. Fixing the door fixes the fixture's root cause, not just
its symptom.

**Who else is waiting:** `build/spike/python-slateos.elf` — the CPython this
tree already boots — references `setitimer`, `getitimer`, `alarm`,
`timer_create`, `timer_settime` and `timer_delete`. Lane B audited that rather
than assuming it, and stated the method's limit honestly (a byte search of the
linked binary, which shows the symbols are present, not that each is called on
a live path).

**Numbers 1069 and 1070**, the next free slots after 1068, per the file's
standing rule that numbers are never recycled.
