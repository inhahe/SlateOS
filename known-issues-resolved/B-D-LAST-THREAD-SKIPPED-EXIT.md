### [D] B-D-LAST-THREAD-SKIPPED-EXIT — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/pthread.rs` — `pthread_exit`, `launch`
(`LIVE_THREADS`); `posix/src/process.rs` — `fork`'s child.

**In short:** when the last thread of a program ends by calling
`pthread_exit` -- because `main` called it and left its threads to finish --
POSIX says the program then ends exactly as if `exit(0)` had been called: its
`atexit` handlers and static destructors run and its buffered output is
written. Ours ended in the kernel instead, so none of that happened, and a
program whose threads wrote through `printf` lost whatever was still
buffered.

**What it was.** `pthread_exit` always issued `SYS_THREAD_EXIT`, and the
kernel ends a process whose last thread exits; nothing in the C library knew
which thread was the last.

**Fix.** glibc's shape (`__nptl_nthreads`): a count of the process's running
threads, 1 for the initial thread, raised by `pthread_create`'s `launch` *before*
`SYS_THREAD_CREATE` (so a new thread that ends at once cannot look like the
last) and lowered by `pthread_exit` after the thread's destructors. The thread
that lowers it to zero calls `exit(0)`. `fork`'s child resets it to 1. Host
tests pin the counting (`only_the_last_thread_is_the_last`, the ordering case,
saturation at zero).

**Still not covered:** a thread the kernel kills on its own (an unhandled
fault in that thread alone) never reaches `pthread_exit`, so it is never
subtracted, and when the others have all called `pthread_exit` the kernel ends
the process as before, without `exit`'s work. That is the old behaviour, and
no worse; covering it needs the kernel to tell the process a thread died.
