### D-PTHREAD-SLOT-PUBLISH-RACE. A child thread can outrun the publication of its own `ThreadSlot` — 2026-07-30 — FIXED 2026-09-26

**Fixed 2026-09-26 (shape 1, below).** `pthread_create` claims and fills the
slot before `SYS_THREAD_CREATE`, and leaves the slot's address in the new
thread's per-thread block (`PerThread::thread_slot`), so the thread reaches
its slot without a lookup by task id. A thread that exits before its creator
has published its id waits for that store (a few of the creator's
instructions) before it lets go of the slot, so a slot is never released
under a creator about to write it. `pthread_getattr_np` of the calling thread
answers from the same slot, which exists before the thread runs -- Rust's std
asks as the thread starts, and a lookup by id then found nothing and reported
the main thread's stack. The table also stopped being 64 slots: it grows a
chunk at a time (`B-D-PTHREAD-CREATE-IGNORED-ITS-ATTRIBUTE`).

**Where:** `posix/src/pthread.rs`, `pthread_create`. The sequence is
`mmap` the combined stack+TLS region → `SYS_THREAD_CREATE` → *then*
`store_thread_info(task_id, stack_base, stack_size, map_size, detached)`.
The `task_id` only exists after the syscall returns, so the slot cannot
be published before the child is runnable.

**Effect:** the child is scheduled the instant `SYS_THREAD_CREATE`
returns in the kernel, so on a multi-CPU (or preempting) system it can
reach `pthread_exit` — which looks its own slot up to decide who frees
the mapping — *before* the parent has written that slot. Consequences,
all rare but real:

- A **detached** thread that exits inside this window finds no slot, so
  it takes the "joinable" path and leaves its stack+TLS mapping for a
  joiner that will never come: an unbounded leak of
  `DEFAULT_THREAD_STACK_SIZE + PT_TLS reserve` per occurrence.
- The parent's late `store_thread_info` then publishes a slot for a
  task id that has already exited, which a later `pthread_join` would
  arbitrate against a dead thread (and, once task ids wrap, potentially
  against a *different* thread).
- Symmetrically, `pthread_detach` called immediately after
  `pthread_create` can lose to the child.

This is **pre-existing** — it predates the combined stack+TLS mapping
(D-NATIVE-CHILD-THREAD-TLS) and is not caused by it; the TLS work only
widened what leaks from "stack" to "stack + TLS block". It has not been
observed in practice because the current boot self-tests are effectively
single-CPU and every child does enough work to lose the race.

**Proper fix:** publish the slot *before* the child can run, which means
the slot must be keyed by something the parent knows up front rather
than by the kernel's task id. Two workable shapes:

1. **Reserve then fill.** Allocate the `ThreadSlot` before
   `SYS_THREAD_CREATE` in a `RESERVED` state that carries the mapping
   details, and hand the child its slot *index* through the same stack
   words that already carry `arg`, `start` and the thread pointer. The
   child claims its slot by index — no lookup by task id, no window.
   The parent fills in `task_id` afterwards purely for `pthread_join`'s
   benefit, and `pthread_exit` CASes the slot state so exactly one of
   {joiner, self-unmap} frees the mapping.
2. **Gate the child.** Have the child spin/futex-wait on a per-slot
   "published" flag written by the parent before touching any slot
   state. Simpler, but it burns a scheduling slice and still needs the
   slot allocated up front, so shape 1 dominates.

Either way this should land together with the atomic detach/exit
arbitration already described under the detached-thread stack-leak entry
above — they touch the same state and the same window — plus a QEMU
self-test that spawns a detached thread whose start routine is a bare
`return`, in a loop, and asserts the address space stays bounded.

**Discovered:** 2026-07-30, while implementing child-thread ELF TLS
(D-NATIVE-CHILD-THREAD-TLS); reading the reclaim protocol to decide
where the TLS mapping should be freed made the publication order
visible.
