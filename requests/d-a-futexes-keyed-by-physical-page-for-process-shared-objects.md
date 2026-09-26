# D → A: futexes on shared memory, so process-shared semaphores and mutexes can work

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Two processes can share a semaphore or a mutex by putting it in shared
memory — `sem_init(sem, 1, n)` on a `MAP_SHARED` mapping, or a mutex with
`PTHREAD_PROCESS_SHARED`. Here a waiter in one process is never woken by the
other: the kernel's futex queues are keyed by (address space, virtual
address), and the two processes' address spaces differ even when the page is
the same. So libc now refuses these with `ENOTSUP` — glibc's answer where
shared futexes are unsupported — instead of handing out objects that deadlock.
Supporting them needs the kernel half below.

## What is there now

`kernel/src/ipc/futex.rs` keys a waiter by `addr_space` (the PML4's physical
address) and `addr` (the virtual address). The module's own note expects
shared mappings to share a PML4, which was true only while shared memory was
mapped into the kernel address space.

## The ask

Key a futex on a shared (`MAP_SHARED`, or SHM-region) mapping by the
physical page and offset instead — Linux's non-private futex key
(`get_futex_key`: inode and page offset for a file mapping, the page itself
for shared anonymous memory) — and keep the (address space, address) key for
private mappings, which is also the fast path. `SYS_FUTEX_WAIT`/`WAKE` need
no new arguments: the key follows from the mapping. Tell lane D when it lands
and libc will allow `PTHREAD_PROCESS_SHARED` and `sem_init(…, 1, …)` again
(`posix/src/pthread.rs`: `futex_supports_pshared`; `posix/src/semaphore.rs`:
`sem_init`).

I have not touched `kernel/**`.
