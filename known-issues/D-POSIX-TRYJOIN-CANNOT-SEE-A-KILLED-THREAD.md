## D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD — `pthread_tryjoin_np` and `pthread_timedjoin_np` never see a thread killed before it could exit (lane D, 2026-09-28) — **Status: OPEN (blocked on lane A: `requests/d-a-a-thread-join-that-does-not-wait.md`)**

**In short:** a program can ask "has that thread finished?" without waiting
(`pthread_tryjoin_np`), or wait only so long (`pthread_timedjoin_np`). Both
work for a thread that ends normally. A thread that is killed instead -- by
a fault it did not handle -- never gets to say it has ended, so both keep
answering "still running" (`EBUSY`, then `ETIMEDOUT`), while a plain
`pthread_join` would have returned at once with `PTHREAD_CANCELED`.

**Where:** `posix/src/pthread.rs`, `join_readiness`: "exited" is the
thread's slot's `STATE_EXITED`, which the thread sets itself in
`pthread_exit`. The kernel knows better -- `SYS_THREAD_JOIN` reports a
killed thread as `Cancelled` -- but it only answers once the thread is gone,
and waits until then.

**Why it is still open:** the one call that can tell "gone" from "running"
without the thread's help is the kernel's, and it has no form that returns
at once or gives up at a deadline.

**Proper fix:** lane A's join with a deadline (the request proposes a
timeout argument to `SYS_THREAD_JOIN`, 0 meaning "don't wait"); then
`pthread_tryjoin_np` asks it with 0, and `pthread_timedjoin_np` with the
time left, instead of polling the slot every millisecond as it does now.
