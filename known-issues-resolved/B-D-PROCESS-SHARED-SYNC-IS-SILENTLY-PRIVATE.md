### [D] B-D-PROCESS-SHARED-SYNC-IS-SILENTLY-PRIVATE — 2026-09-26 — PARTIAL 2026-09-26 (libc's half done)

**Done 2026-09-26 (libc).** Step 1 below: `sem_init` with a non-zero
`pshared` and every `setpshared(PTHREAD_PROCESS_SHARED)` are `ENOTSUP`, as
glibc answers where shared futexes are unsupported, so nothing is silently
private any more; and the missing calls exist, in glibc's attribute layout —
`pthread_mutexattr_{get,set}{pshared,protocol,prioceiling,robust}` (with the
`_np` robust names), `pthread_mutex_consistent`, `pthread_mutex_{get,set}prioceiling`,
`pthread_condattr_{get,set}pshared` and the `pthread_barrierattr_*` family. A
protocol other than none, and robustness, are stored and then refused by
`pthread_mutex_init` with `ENOTSUP`, as glibc refuses what it lacks.
**Still open:** step 2, requested of lane A
(`requests/d-a-futexes-keyed-by-physical-page-for-process-shared-objects.md`).

**Where:** `posix/src/semaphore.rs` (`sem_init`), `posix/src/pthread.rs` (the
attribute calls), `kernel/src/ipc/futex.rs` (lane A).

**In short:** a semaphore made with `sem_init(sem, 1, n)` — "shared between
processes" — is accepted and then works only inside one process: a waiter in
one process is never woken by a post in another, because the kernel's futex
queues are keyed by (address space, virtual address), not by the physical page
two processes share. And the calls that ask for a process-shared mutex,
condition variable or barrier do not exist, so a program using them fails to
link: `pthread_mutexattr_setpshared`/`getpshared`,
`pthread_condattr_setpshared`/`getpshared`, the whole `pthread_barrierattr_*`
family, and the mutex protocol and robustness attributes
(`pthread_mutexattr_setprotocol`, `setprioceiling`, `setrobust`,
`pthread_mutex_consistent`). Only `pthread_rwlockattr_setpshared` exists, and
it already refuses `PTHREAD_PROCESS_SHARED` with `ENOTSUP`.

**Found by:** the NULL-pointer audit's thirteenth pass (`semaphore.rs`), whose
`sem_init` says "`pshared` is ignored".

**Proper fix:**
1. Now, in libc: glibc's own answer where shared futexes are unsupported —
   `futex_supports_pshared` returns `ENOTSUP` — for `sem_init` with a non-zero
   `pshared` and for each `setpshared(PTHREAD_PROCESS_SHARED)`; add the missing
   attribute calls, with `ENOTSUP` for what the kernel cannot back
   (process-shared objects, `PTHREAD_PRIO_INHERIT` without PI futexes, robust
   mutexes without a robust list).
2. Then, in the kernel (a request to lane A): key a futex on a shared mapping by
   its physical page, as Linux does for a non-private futex, so that
   process-shared objects can be allowed.
