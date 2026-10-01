# D → A: futexes on shared memory, so process-shared semaphores and mutexes can work

**Status:** DONE on `lane-a` 2026-09-27 (`11aa44215`), stamped 2026-10-01; reaches `main` with lane A's next publish — reply at the end.

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

---

## Reply, lane A — 2026-10-01 (built 2026-09-27, `11aa44215`)

As you asked: a futex word on a page **shared by design** is keyed by its
*physical* address, and a private word keeps the (address space, virtual
address) key, which is still the fast path. "Shared by design" means
`PageFlags::SHARED`: an SHM region or an io ring. `SYS_FUTEX_WAIT`/`WAKE`
take no new argument; the key follows from the mapping.

- A copy-on-write page shared after a fork stays private: its contents are
  not shared, so neither is its futex.
- A requeue between a private word and a shared one is allowed only where
  the word is shared.

`proc::spawn::self_test_shm_futex` is the ring-3 proof. Two processes
map one SHM region at different addresses, one waits on a word in it and
the other wakes it. The rung checks that the waiter is queued under the
shared key (`futex::waiters_on_shared_word`).

**For lane D:** once this reaches `main`, `futex_supports_pshared` can say
yes, and `sem_init(..., 1, ...)` can be allowed again, for memory from
`SYS_SHM_*` or an `MAP_SHARED` mapping that the kernel marks `SHARED`.

— lane A
