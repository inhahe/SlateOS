## B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING (lane B, 2026-09-07)

**Status: FIXED 2026-09-12** (lane B). `alarm`, `ualarm`, `setitimer` and
`getitimer` now reach the kernel's real interval timer through
`SYS_ITIMER_SET` (1069) and `SYS_ITIMER_GET` (1070), so a `SIGALRM` actually
arrives. `ITIMER_VIRTUAL` and `ITIMER_PROF` are **refused with ENOSYS** rather
than answered out of a table this module kept -- the `setgroups` call again
(design-decisions.md §1004): a caller told ENOSYS can choose a fallback, a
caller told 0 cannot.

**What unblocked it, and the lesson in that.** This entry's own conclusion was
"implement it, once lane A exposes a native number", and it sat waiting. Lane A
had already done so **on 2026-09-09, the same day it was asked** -- and said so
in a notice, which also pointed out that the request file's own second line
recorded the closure. The entry went on saying `blocked` for three days because
nothing re-read it after the ground moved. That is precisely the failure lane A
described in the same exchange: *a statement that was true when written, in a
document read as present tense.* Worth noting that no amount of care at the
keyboard prevents it; only revisiting does.

**On the host there is a simulator**, the same split `pipe.rs` uses, and it is
not a return of this bug. The bug was that the *target* stored a value, armed
nothing, and reported success. Now the target really arms; the host keeps a
per-thread nanosecond store so `cargo test` still exercises the validation
order and the `Timeval`↔nanosecond conversions either side of it, which is the
libc-side logic and the only part a host test can be about. Whether a signal
actually arrives is a kernel question, and the kernel has its own self-test for
1069/1070.

Two tests changed rather than being deleted, both because they encoded the
three-timer fiction: `test_setitimer_valid_which` asserted all three `which`
values return 0, and `test_getitimer_per_timer_type_isolation` asserted that
reading an unset virtual timer reports zeros. Zeros are a plausible answer --
"no timer is set" -- and a caller cannot tell them from "this kind of timer
does not exist here", which is how the old behaviour stayed invisible. Both now
assert the refusal *and* the errno, because ENOSYS and not EINVAL is what
distinguishes "not built" from "you asked wrongly". `cargo test -p posix
--target x86_64-pc-windows-gnu`: 20702 passed, 0 failed.

The description below is kept in the tense it was written in.

**In short:** the functions a program calls to ask for "wake me in 5 seconds"
(`timer_create`, `timer_settime`) check their arguments, report success, and set
no timer. Nothing ever fires. A program that schedules a timeout and waits for
it waits forever.

**Where.** `posix/src/time.rs`, the "POSIX per-process timers" section.

**Why it is being logged now, having been true for a long time.** The reason
recorded next to the code was *"our OS does not deliver Unix signals"*, and
while that was true this was not a bug but an honest consequence of the
architecture -- there was no mechanism a timer could have used. That reason has
stopped being true. `posix/src/signal.rs` registers `__signal_trampoline` with
the kernel in `__libc_start_main` (`crt.rs:700`), the kernel delivers pending
unblocked signals through it, and handlers registered by `signal`/`sigaction`
really run. So the delivery half exists and the gap is now narrow and specific:
**nothing asks the kernel for a timer.** `SIGALRM` has a way to arrive and no
one to send it.

That changes its character. It was a documented architectural limit; it is now
a function that reports it did something it did not do -- the same family as
`B-POSIX-SETGROUPS-REPORTS-SUCCESS-WITHOUT-CHANGING-ANY-GROUPS`, fixed earlier
the same day.

**Why it is not simply flipped to `ENOSYS` the way `setgroups` was.** The
audit that made `setgroups` easy does not come out the same way here. That one
had *no callers anywhere in the tree, in any language*, so nothing could break.
These have a plausible caller population -- the comment names the case, programs
that create a timer at startup for profiling or a heartbeat and would fail at
startup if the call errored -- and CPython, this tree's largest libc consumer,
exposes them through `signal.setitimer` and `time`. That population has **not**
been audited, and this entry deliberately does not recommend a direction until
it has been, because "nobody has audited it" is a statement about the auditing
and not about the hazard. Doing the audit is the next step, not the decision.

### The audit, done 2026-09-09 — and it comes out the other way

**In the tree: no callers.** Across every file type, the only match outside
`posix/` and `kernel/` is a doc comment in `apps/alarmclock` about dismissing
an alarm clock, which is not this.

**Outside the tree: a real one.** The built interpreter,
`build/spike/python-slateos.elf`, references `setitimer`, `getitimer`,
`alarm`, `timer_create`, `timer_settime` and `timer_delete` -- all six.
CPython is this tree's largest libc consumer and it already *runs* on SlateOS.
So the population is not empty, and `ENOSYS` is not the free move it was for
`setgroups`. (Method, stated because it is weaker than a symbol table: a byte
search of the linked binary for each name. It shows the symbols are present,
not that every one is called on a live path. `nm` is not available here.)

**Which way that cuts is less obvious than it looks.** Silent success is not
obviously the safer option for a *timeout*: `signal.alarm(5)` succeeding and
never firing converts a bounded wait into an unbounded one, which is the hang
class that cost this lane two hours the same week. A loud failure at least
lets a caller choose a fallback. But that is an argument for *fixing* it, not
for breaking a running interpreter, and it is a user-visible policy change in
a language runtime rather than a tidy-up.

**So the direction is neither: implement it.** The kernel already has
`proc/itimer.rs`, which backs `alarm` and `setitimer(ITIMER_REAL)` with a real
`SIGALRM` and is wired into `kernel/src/syscall/linux.rs` -- but has no native
syscall number, exactly as `setgroups` and `chroot` did. Asked of lane A in
`requests/b-a-expose-the-interval-timer-natively-so-alarm-can-fire.md`. When
that number exists this stops being a stub without any of the above needing to
be decided.

**Two siblings, same premise -- both FIXED 2026-09-09, same day.**
`pause()` slept a second and returned `EINTR`, and `sigsuspend()` returned
`EINTR` immediately: both reported "a signal was delivered" when none was.
Neither needed the kernel. `signal.rs` now keeps a delivery counter, bumped
whenever a registered handler runs, and both wait on it -- because a delivered
signal otherwise leaves *no trace a waiter can observe* (delivery is
asynchronous, and `SYS_SIGNAL_PENDING` reports signals blocked and queued,
which is the opposite set).

Lane A had already built the other half and said so:
`kernel/src/proc/thread.rs` posts `SIGCHLD` to a native parent specifically so
one "parked in `sigsuspend()`/`pause()`" wakes, remarking that "without this
the parent livelocks in sigsuspend". Nothing had ever been parked there,
because the libc side never waited -- so a carefully built kernel path had no
consumer.

Two details worth keeping. The counter counts handler **invocations**, not
deliveries, which is what makes an ignored signal correctly *not* end a
`pause` -- POSIX's rule, arrived at by construction rather than by a special
case. And it is atomic, not for threading (the `process_global!` macro assumes
a single-threaded target) but because the increment runs *in signal-delivery
context*: a second delivery landing inside a read-modify-write would lose an
update, and a lost update is a missed wake, not a wrong number.

**The two honest options, for when it is:** arm a real kernel timer (the kernel
already has the delivery path -- `kernel/src/proc/signal.rs` mentions
`ITIMER_REAL`), or fail loudly so callers can choose a fallback. Succeeding
silently is the one option that is wrong in every case.
