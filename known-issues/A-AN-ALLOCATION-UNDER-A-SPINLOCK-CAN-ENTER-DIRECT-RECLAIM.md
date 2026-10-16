## A-AN-ALLOCATION-UNDER-A-SPINLOCK-CAN-ENTER-DIRECT-RECLAIM (lane A, 2026-10-16) — OPEN

**Status:** OPEN (lane A). Raised by the Opus review of lane A's batch
284a07018..0713e88ab, about `ipc::semaphore`; the gap is kernel-wide.

**In short:** when the kernel runs short of memory, the code that hands out
memory first tries to free some (*direct reclaim*: writing pages out to swap,
which can wait for a disk and lets other tasks run meanwhile). Code that holds
a spinlock -- a lock that keeps its holder on its processor and forbids being
switched away -- may not wait like that. Yet nearly every spinlock-protected
table in the kernel grows its contents while the lock is held, and nothing
tells the memory code it is being asked from such a place. Linux solves this
with "atomic" allocations that never reclaim and draw on a small reserve kept
for them; this kernel has neither.

### What happens today

An allocation made under a spinlock (`crate::sync::Mutex`,
`PreemptSpinMutex`) that finds no free frame runs `frame::alloc_order_inner`'s
slow path: pressure notifiers, `swap::try_reclaim` (which yields between
batches and may do swap I/O), compaction, the OOM killer. A yield or a wait
there switches away with the lock held -- what the boot's switch-under-lock
check counts as a defect, and has not yet seen happen -- and the I/O and the
OOM killer's process teardown run with the lock held, and can reach the lock
again.

The swap code's own locks are already covered: a holder of `SWAP` or
`RECLAIM` is marked, and its allocations fail at once
(`swap::reclaim_forbidden`, 2026-10-16). Every other spinlock is not.

### The fix

1. The frame allocator treats a caller with preemption disabled as atomic:
   no reclaim, no compaction, no OOM killer -- Linux's `GFP_ATOMIC`,
   inferred from the preempt count rather than passed by every caller.
2. A reserve of frames only such callers may take, refilled by kswapd, so
   that refusing them does not turn every brief shortage into a failed
   allocation and, where the allocation is infallible, a panic.
3. Where a table's growth under its lock is on a hot path, the room is made
   before the lock is taken, as `swap::reserve_for_area` and
   `swap::list_devices` do.

(1) without (2) would trade a hang for panics under pressure, which is why
this is not a one-line change.

### Where

`kernel/src/mm/frame.rs` (`alloc_order_inner`, `alloc_order_constrained_inner`),
`kernel/src/mm/kswapd.rs`; the callers are every spinlock-protected
collection, `ipc::semaphore`'s `SEM_TABLE` among them.
