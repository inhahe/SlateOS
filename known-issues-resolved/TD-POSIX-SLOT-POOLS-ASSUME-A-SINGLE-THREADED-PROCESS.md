### TD-POSIX-SLOT-POOLS-ASSUME-A-SINGLE-THREADED-PROCESS. Every fixed-size table in `posix/` claims slots with an unsynchronised check-then-set — LOGGED 2026-08-13 — FIXED 2026-08-13

**Where:** the `process_global!` tables and their `alloc_*` helpers in
`posix/src/`: `aio.rs` (~line 283), `dirent.rs` (~709, ~869), `epoll.rs` (~173,
~1131, ~1829, ~2714), `mqueue.rs` (~326, ~357, ~610), `semaphore.rs` (~600),
`stdio.rs` (~169), `sysv_msg.rs` (~327, ~532), `sysv_sem.rs` (~296),
`sysv_shm.rs` (~311). Roughly 15 allocation sites across 10 modules.

**The debt.** All of them claim a slot the same way:

```rust
for (i, inst) in table.iter_mut().enumerate() {
    if !inst.in_use {       // check ...
        inst.in_use = true; // ... then set, with nothing held
        return Some(i);
    }
}
```

This is safe only under the assumption written into
`posix/src/perprocess.rs`: *"the target links this crate into one single-threaded
process"*. On the host that assumption is enforced — `process_global!` expands
to a `thread_local!`, so each test thread gets its own table. On the target
(`target_os = "none"`) it expands to a bare `static mut` and the assumption is
load-bearing.

**Why it's questionable.** The crate implements `pthread_create`. If a target
process ever runs two threads that both call, say, `opendir` or `epoll_create`,
they can claim the same slot and then corrupt each other's state — exactly the
failure that `B-FTS-INSTANCE-POOL-IS-SHARED-ACROSS-THREADS` produced on the host
once `fts.rs` was found not to be using `process_global!`. Several of these are
worse than `fts` because allocation is *compound*: `sysv_msg::alloc_queue(key)`,
`sysv_sem::alloc_set(key, ..)` and `sysv_shm::alloc_segment(key, ..)` scan for an
existing entry with a matching key and then allocate if absent, so even an atomic
per-slot claim would not make them correct — two threads could each create a
segment for the same key.

**Correction to the survey above.** The original entry counted ten unlocked
modules. On starting the work, five of them — `mqueue`, `semaphore`,
`sysv_msg`, `sysv_sem`, `sysv_shm` — turned out to *already* serialise their
scans, each with its own hand-rolled `AtomicBool` + `lock_acquire` /
`lock_release` / `struct Guard`. So the real debt was four genuinely-unlocked
modules (`aio`, `dirent`, `epoll`, `stdio`) plus five near-identical copies of
the same twenty lines of locking. The copies had already drifted (`AcqRel` vs
`Acquire` on the exchange, differing guard names), which is the usual argument
for the shared primitive.

**Fix (2026-08-13).** `perprocess.rs` gained `PoolLock` / `PoolGuard` /
`lock_pool()`: a spin lock whose guard releases on `Drop`, so the early
`return` out of the middle of a claim loop — the shape every one of these pools
is written in — needs no manual unlock. All fifteen sites now run
scan-and-claim inside one critical section, and the five hand-rolled locks were
deleted in favour of it. Rationale in **design-decisions.md §301**; the short
version:

- **One guard over the whole scan, not a per-slot `compare_exchange`.** A CAS
  on `in_use` fixes the simple pools but cannot express the compound
  find-or-create ones (`sysv_*::alloc_*(key)`, the getdents cache), where the
  lookup and the claim must be one indivisible step.
- **The lock covers scans, not the objects.** Allocate, release and
  find-by-key take it; `with_instance_mut`, `readdir`, `epoll_ctl` and every
  other by-index accessor stay unlocked, because POSIX already makes
  concurrent use of one `DIR *` / `FILE *` the caller's problem. Each such
  accessor now carries a comment saying so, replacing the stale
  `// SAFETY: single-threaded` that used to be there.
- **Releases take the lock too.** Clearing `in_use` outside it is a plain data
  race against a concurrent claim reading it.
- **The lock's scope must match its table's scope.** `process_global!` table →
  `process_global!` lock (declared in the same block); plain `static mut` table
  → plain `static` lock. `mqueue` and `semaphore` were found with a plain
  `static` lock over a `process_global!` table — over-broad rather than
  under-broad, so safe, but on the host it couples every test thread to every
  other, and a spin lock has no poisoning: a panic inside the critical section
  would wedge unrelated later tests. Both moved inside the `process_global!`
  block.

**Two structural changes fell out of it:**

- `dirent`'s getdents cache was split into `claim_getdents_cache()` (takes the
  slot with `fd = -1`, so no concurrent `find_getdents_cache` can match it) and
  `publish_getdents_cache(slot, fd)` (called once the `SYS_FS_LIST_DIR`
  snapshot is filled). That syscall is far too slow to hold a process-wide lock
  across.
- `aio`'s finders were split into `_locked` variants, because the spin lock is
  not reentrant and `store_aio_record` calls the finder from inside its own
  critical section. Without the split it would have deadlocked against itself
  on the first concurrent `aio_read`.

**Regression test:** `perprocess.rs`'s
`a_scan_and_claim_under_the_lock_never_hands_out_a_slot_twice` — 8 threads × 8
claims over 64 slots. Its first version passed with `lock_pool` removed,
because the `Mutex<Vec<usize>>` collecting the results was serialising the
threads all by itself; the version in the tree adds a `std::sync::Barrier`, a
`yield_now()` between the read of `in_use` and the write, and per-thread
private `Vec`s merged after `join`. With the lock removed it now fails as
`left: 20, right: 64`. A companion test checks the guard actually releases on
an early `return` by reading the flag directly, so a regression fails rather
than hangs.

**Residual, accepted.** Two threads calling `getdents64` on the *same* fd
simultaneously can each end up with their own cache slot, each with its own
position — a consequence of the claim/publish split above. That is concurrent
use of one fd, already the caller's problem, and it is memory-safe.
