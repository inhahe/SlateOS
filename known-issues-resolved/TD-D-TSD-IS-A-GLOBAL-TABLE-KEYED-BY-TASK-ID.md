### [D] TD-D-TSD-IS-A-GLOBAL-TABLE-KEYED-BY-TASK-ID — 2026-09-26 — FIXED 2026-09-26

**Fixed 2026-09-26**, as proposed below: the keys are a table of 128
(musl's `PTHREAD_KEYS_MAX`, and what `sysconf(_SC_THREAD_KEYS_MAX)` reports),
each with a sequence number and a destructor; each thread's values are in
blocks of 32 hanging off its per-thread block, allocated as it first sets a
key in each and freed when it exits; `pthread_getspecific` is loads and a
compare. A deleted key's index is reused, a value set under it before reads
as NULL, and deleting a key not in use is `EINVAL`, as in glibc. The
destructor sweep repeats only while destructors set values again, up to
four times.

**Where:** `posix/src/pthread.rs` — `TSD_TABLE` and `pthread_key_create`,
`pthread_key_delete`, `pthread_getspecific`, `pthread_setspecific`.

**What it is.** Thread-specific data lives in one process-wide table of 64
rows, one per thread that has set a key, keyed by kernel task id, under a spin
lock. Every `pthread_getspecific` makes a `SYS_TASK_ID` syscall, takes the
lock and scans up to 64 rows -- for what glibc does with two loads. A 65th
thread holding values at once gets `ENOMEM` from `pthread_setspecific`. And
keys are never reused: `pthread_key_delete` leaves the index taken, so a
program that creates and deletes keys runs out after 64 creations (`EAGAIN`)
with none alive. The row count was the thread table's until that table
learned to grow (`B-D-PTHREAD-CREATE-IGNORED-ITS-ATTRIBUTE`).

**Proper fix.** glibc's design: each thread's values in its own per-thread
block (an inline first block, further blocks allocated on first use), keys
carrying a sequence number so that a deleted-and-recreated key reads NULL in a
thread holding a stale value, and neither a lock nor a syscall on
`pthread_getspecific`. The inline block goes in `PerThread`, within its
2048-byte budget (`the_block_stays_small_enough_to_ride_in_every_thread`).
