## 1193. SIGRTMIN is 34, and signals 32 and 33 are the C library's, as glibc's -- the room `SIGEV_THREAD` timers need

**Date:** 2026-10-16
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a program that asks for a timer to run a function in a new
thread (`SIGEV_THREAD`) needs the C library to keep a signal for itself: a
helper thread waits for that signal and starts the function's thread at
each expiry. glibc keeps signals 32 and 33 for such purposes and tells
programs that their real-time signals start at 34 (`SIGRTMIN`). This
library told programs 32 and kept none. It now does what glibc does: 32 and
33 are its own -- a program cannot put them in a set, give them an action,
or block them -- and `SIGRTMIN` is 34. A program that hard-codes 32 as its
first real-time signal would now be refused where it worked before; one
that uses `SIGRTMIN`, as `<signal.h>` asks, sees no change but the number.

**Where:** `posix/src/signal.rs` (`SIGRTMIN`, `SIGTIMER`, `SIGSETXID`,
`is_internal_signal`, `sigaddset`, `sigdelset`, `sigfillset`, `sigaction`,
`sigprocmask`, `default_action`), `posix/src/time.rs` (`thread_timers`).

### The alternatives

1. **Keep `SIGRTMIN` at 32 and take a signal from the top (64) for the
   timers.** No program's numbering moves. But a program that uses
   `SIGRTMAX` would collide with the library's own, and this library's
   numbering would still differ from glibc's -- the oracle every other
   answer here is held to (design-decisions 1189) -- in `strsignal`, in the
   BSD masks, in every program that prints a real-time signal's number.
2. **Glibc's numbering (chosen).** 32 is the timers' signal (`SIGTIMER`,
   which glibc makes `SIGCANCEL`), 33 kept unused (`SIGSETXID`'s number) so
   that real-time signals start where glibc's do. `pthread_kill` and
   `pthread_sigqueue` already refused 32 and 33 as glibc's do, so the
   library was inconsistent with itself before.
3. **musl's: a thread per timer, running the function itself.** musl keeps
   32-34 and starts `SIGRTMIN` at 35, which would move numbers further from
   glibc's.

### How the timers then work (glibc's model, not musl's)

One helper thread per process, made at the first `SIGEV_THREAD` timer, with
every signal blocked; each timer's kernel timer sends 32 to that thread
alone (`SIGEV_THREAD_ID`), carrying the address of the library's record of
the timer; for each expiry the helper makes a new detached thread with the
event's attributes, and that thread runs the function -- with every signal
blocked, as glibc's run it (measured on Linux 6.6.87). musl instead runs the
function on one thread per timer, so one timer's calls never overlap and a
thread's state carries from one call to the next; POSIX's wording ("as if it
were the start routine of a newly created thread") reads as glibc's does,
and glibc is the oracle.

### Reverse it by

Setting `SIGRTMIN` back to 32 and moving `SIGTIMER` to 64, in
`posix/src/signal.rs`; nothing else names the numbers.
