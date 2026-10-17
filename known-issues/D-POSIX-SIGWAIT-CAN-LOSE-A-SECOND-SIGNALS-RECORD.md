## D-POSIX-SIGWAIT-CAN-LOSE-A-SECOND-SIGNALS-RECORD — a second signal that reaches a `sigwait` in the moment it is taking a first is re-sent as a bare number, losing its sender, value and thread (lane D, 2026-10-17) — OPEN
**Status:** OPEN -- waiting on lane A (`requests/d-a-a-native-call-to-take-a-pending-signal-of-a-set.md`).

**In short:** a thread waiting in `sigwait` (or `sigtimedwait` or
`sigwaitinfo`) takes its signal by letting the kernel deliver it and
catching it in the library's dispatch. The library's timer thread for
`SIGEV_THREAD` timers takes every expiry the same way. If a second
signal of the set is delivered in the moment between the first's delivery
and the library closing the set, the waiter already holds the first.
The library then cannot give the second back to the kernel as it was, so
it re-sends the bare number to its own process. The second signal's
sender, code and value are lost, and a signal aimed at the thread becomes
one for the process. For a timer's expiry, the value is what names the
timer, so that expiry's notification does not run.

Until round 205 this happened whenever two signals of the set were
pending together: the set stayed open until the waiting thread itself
withdrew it, and the kernel delivered the second at the dispatch's
return (Opus's review of lane D's round 205: two `SIGEV_THREAD` timers
expiring together lost the second). Now the dispatch closes the set as
it hands the first over, so a signal already pending stays pending. What
is left needs a signal posted in that moment and delivered from an
interrupt taken there, because the kernel does not block a signal while
it delivers it.

**Where:** `posix/src/signal.rs` -- `hand_to_a_waiter`'s second pass (the
`extra` bits) and `accept`, which re-sends them through `keep_pending`.

**The proper fix:** lane A's native call that takes a pending signal of
a set with its record, as `rt_sigtimedwait` does for Linux programs (the
request above). `sigtimedwait` and the timer thread then use it, and the
table of waiters and its `extra` go (design-decisions §1158's first
alternative).
