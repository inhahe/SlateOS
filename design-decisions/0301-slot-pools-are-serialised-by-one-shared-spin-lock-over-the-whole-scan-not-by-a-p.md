## §301 — Slot pools are serialised by one shared spin lock over the whole scan, not by a per-slot atomic, and the lock is scoped exactly like the table

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B (POSIX & userland)
**Affects:** `posix/src/perprocess.rs` (the primitive), `posix/src/{dirent,stdio,aio,epoll,mqueue,semaphore,sysv_msg,sysv_sem,sysv_shm}.rs` (the pools)

### The problem

The POSIX layer keeps about fifteen fixed-size slot pools — `DIR` handles,
`FILE` handles, `popen` children, `aiocb` records, epoll/timerfd/inotify
instances, the getdents caches, POSIX message queues and their descriptors,
named semaphores, and the three System V tables. Ten of them claimed a slot
like this:

```rust
for (i, slot) in table.iter_mut().enumerate() {
    if !slot.in_use {       // check ...
        slot.in_use = true; // ... then set, with nothing held
        return Some(i);
    }
}
```

Check-then-set with nothing held is correct in a single-threaded process, and
every one of these modules carried a `// SAFETY: single-threaded posix layer`
comment saying so. That comment stopped being true when this crate grew a
`pthread_create`. Two threads calling `opendir` at the same instant can both
read `in_use == false` for slot 3 and both be handed slot 3, after which each
silently scribbles on the other's directory stream. Nothing in the type system
or the test suite catches it: single-threaded tests pass forever.

The remaining five modules (`mqueue`, `semaphore`, and the three System V
ones) already locked correctly — but each with its own hand-rolled
`AtomicBool` + `lock_acquire`/`lock_release`/`struct Guard`, five near-identical
copies of the same twenty lines.

### Decision 1 — one lock over the whole scan, not a per-slot `compare_exchange`

`PoolLock` is a spin lock; `lock_pool()` returns a `PoolGuard` that releases on
`Drop`; the entire scan-and-claim runs inside one critical section.

The obvious cheaper alternative is a `compare_exchange` on each slot's `in_use`
flag: no lock, no contention between threads claiming *different* slots, and it
makes the simple pools correct. It was rejected because it does not cover the
*compound* pools. `sysv_msg::alloc_queue(key)`, `sysv_sem::alloc_set(key, …)`
and `sysv_shm::alloc_segment(key, …)` first scan for an existing entry with a
matching key and allocate only if there is none. A per-slot atomic claim keeps
two threads from taking the same *slot*, but does nothing to stop them each
creating a *separate* segment for the same key — the lookup and the claim have
to be one indivisible step, and a per-slot CAS cannot express that. The
getdents cache has the same find-or-create shape.

So the choice was between one primitive that covers every pool, or two
primitives where the weaker one silently does not apply to a third of them and
the next person to add a pool has to work out which. Allocation is not a hot
path — once per `opendir`, per `epoll_create`, per `mq_open` — the critical
section is a bounded scan of a small array, and the crate is `no_std` on the
target with no blocking primitive available. Uniformity wins; the contention
cost is not measurable at these call rates.

### Decision 2 — the lock covers *scans*, not the objects in the pool

Only allocate, release, and find-by-key take the lock. Once a caller holds the
`DIR *`, `mqd_t`, or instance index that names its own slot, it reaches that
slot unlocked: `with_instance_mut`, `readdir`, `epoll_ctl` and friends stay
lock-free.

This is a real boundary, not laziness. POSIX already makes concurrent use of a
single `DIR *` or `FILE *` by two threads the caller's problem, so there is no
contract to uphold; and taking a process-wide lock on every `readdir` would
serialise unrelated directory streams for nothing. The lock is on the *pool*,
not on the objects in it — and each unlocked accessor now says so in place of
the old "single-threaded" comment, so the boundary is documented where someone
would otherwise "fix" it.

Two consequences are accepted rather than solved:

- **Releases take the lock too.** Clearing `in_use` outside it is a plain data
  race against a concurrent claim reading it, and could let a slot be reissued
  before the release is visible. Every `*_close` therefore takes the guard even
  though it, too, names its slot by index.
- **The getdents cache can still end up with two slots for one fd.** Its
  `SYS_FS_LIST_DIR` snapshot is far too slow to hold a process-wide lock
  across, so the pool uses claim-then-publish: `claim_getdents_cache()` takes
  the slot with `fd = -1` (so no concurrent `find_getdents_cache` can match it)
  and `publish_getdents_cache(slot, fd)` sets the real fd once the buffer is
  filled. Two threads calling `getdents64` on the *same* fd simultaneously can
  therefore each get their own cache slot, each with its own position. That is
  concurrent use of one fd — already the caller's problem — and it is
  memory-safe, which is what the lock is for.

### Decision 3 — the lock's scope must match its table's scope

The pools are declared two ways, and the lock has to follow:

| the table is | the lock must be | why |
|---|---|---|
| `process_global!` (`epoll`, `mqueue`, `semaphore`, `aio`, `stdio`'s popen store) | `process_global!`, declared in the same block | on the host each test thread owns its own table, so a shared lock serialises threads that cannot collide |
| a plain `static mut` (`dirent`, `stdio`'s `FILE` pool, the three System V tables) | a plain `static` | the table really is shared on both builds, so a per-thread lock would guard nothing |

Getting this backwards is silent — a too-narrow lock compiles, passes every
single-threaded test, and protects nothing. `mqueue` and `semaphore` were both
found with the *other* mismatch: a plain `static` lock over a `process_global!`
table. That direction is safe (over-broad, not under-broad) and correct on the
target, where `process_global!` is itself a `static mut`. It was still fixed,
because a spin lock has no poisoning and no unwinding path: one host test that
panics inside the critical section of a *shared* lock hangs every later test
that touches the pool, including tests that share no data with it. Declaring
the lock inside the `process_global!` block, beside its table, is what keeps
the two from drifting apart later.

The one deliberate exception is `epoll`'s `event_scratch_lock`, which guards a
~8 KiB *buffer* rather than a slot table and is held across the
`SYS_FS_WATCH_READ` syscall. It has to be: the syscall is what fills the
buffer, and the records are parsed straight out of it, so releasing the lock in
between would let another thread refill it mid-parse. It is re-taken per batch
so a long drain does not monopolise the buffer.

### Alternatives considered

**Leave it single-threaded and document the restriction.** The pools predate
`pthread_create`; one could declare that the POSIX layer's allocators are
simply not thread-safe. Rejected: the whole point of this layer is that
unmodified Linux binaries behave as they do on Linux, and on Linux `opendir`
from two threads is ordinary, unremarkable code. A documented restriction that
real programs violate by default is not a restriction, it is a latent bug.

**A `Mutex`-like type with poisoning.** No `std` on the target, and poisoning
implies unwinding, which the kernel-side build does not have. The spin lock's
failure mode under a panic is a hang rather than a poisoned error — which is
exactly why Decision 3 pushes every lock to the narrowest scope its table
allows.

**Keep the five hand-rolled locks.** They worked. But five copies of the same
twenty lines is five places for the next fix to be applied to four of them, and
the copies had already drifted (`AcqRel` vs `Acquire` on the exchange, differing
guard names, and the two scope mismatches above). Collapsing them onto the
shared primitive is what surfaced the mismatches in the first place.

### How the test has teeth

`a_scan_and_claim_under_the_lock_never_hands_out_a_slot_twice` is worth
describing, because its *first* version proved nothing: it passed with
`lock_pool` deliberately removed. The `Mutex<Vec<usize>>` used to collect the
claimed indices was serialising the worker threads all by itself, so they never
actually raced. The version in the tree fixes that three ways — a
`std::sync::Barrier` so all eight threads enter the claim loop together, a
`yield_now()` between the read of `in_use` and the write to widen the window,
and per-thread private `Vec`s merged only after `join`. With the lock removed
it now fails as `left: 20, right: 64`. A concurrency test that has never been
observed to fail is not evidence of anything.

### How to reverse

`PoolLock` is a private crate primitive with no ABI surface; the guard is
acquired at the top of each allocator. Reverting means deleting the
`let _guard = …` lines and the lock statics. The claim/publish split in
`dirent`'s getdents cache is the one structural change that would need undoing
separately.
