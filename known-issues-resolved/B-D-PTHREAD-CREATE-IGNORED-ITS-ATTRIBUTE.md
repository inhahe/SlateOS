### [D] B-D-PTHREAD-CREATE-IGNORED-ITS-ATTRIBUTE — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/pthread.rs` — `pthread_create`, the thread table,
`pthread_attr_init`; `posix/src/perthread.rs`.

**What it was.** `pthread_create` took its attribute as `_attr` and never
read it. Every thread got a 64 KiB stack whatever size it asked for -- Rust's
std asks for 2 MiB -- with no guard page below it, so a thread that used more
than 64 KiB wrote into whatever was mapped below its stack, silently. A
thread asked to start `PTHREAD_CREATE_DETACHED` started joinable, and since
nothing joined it, its stack leaked. A stack the caller supplied
(`pthread_attr_setstack`) was ignored. And the thread table held 64 threads:
the 65th ran untracked, its mapping leaked on join, `pthread_detach` told the
caller it did not exist, and `pthread_getattr_np` reported the main thread's
stack for it. `pthread_attr_init` recorded a guard of 0, where glibc records
a page. Found reading `pthread_create` for the aio notification thread, which
must be created detached.

Two smaller faults beside it: `find_slot` matched the sentinels, so
`pthread_detach(0)` found an empty slot, marked it detached and answered 0;
and the slot was published only after the thread started
(`D-PTHREAD-SLOT-PUBLISH-RACE`).

**Fix.** `pthread_create` reads the attribute: the stack size (rounded up to
pages), a guard of the attribute's size below the stack, mapped inaccessible
(one page by default, as glibc and musl), a caller's stack as given with the
TLS part in a small mapping of its own (never unmapping the caller's memory),
and the detach state. The table grows a chunk of 64 slots at a time and never
shrinks, so a slot's address is stable and can be handed to its thread.
Host tests cover the attribute reading, the layout arithmetic, the table's
growth and the sentinels; `services/ctest-pthread` checks it all in ring 3,
once lane A runs it (`requests/d-a-run-the-ctest-pthread-fixture.md`).
Design choices in `design-decisions.md` §1111.
