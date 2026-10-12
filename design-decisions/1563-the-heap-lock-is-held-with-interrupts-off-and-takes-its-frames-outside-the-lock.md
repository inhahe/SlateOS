## 1563. The heap lock is held with interrupts off, and the heap takes its frames outside the lock

**Date:** 2026-10-09 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the kernel's memory allocator (its heap) is protected by a lock
that, until now, turned off task switching but not interrupts. Code that runs
in an interrupt -- a timer waking a sleeping task -- also allocates memory. So
when an interrupt arrived on a processor that was in the middle of
allocating, the interrupt waited for the lock that the code it had
interrupted was holding, and that code could never continue: the machine
stopped. This is the intermittent hang seen on two-processor boots since
2026-10-08 ("a hang at the native device-door test"), caught on 2026-10-09
with every processor's registers and stack. The lock now turns interrupts
off while it is held, as Linux's equivalent does. The allocator also no
longer asks the physical-memory allocator for more memory while holding its
lock, because that request can, when memory is short, need every other
processor to answer an interrupt.

**What happened, from the capture** (`hangcap.py` on a two-CPU fast boot of
lane-a-wip; symbols by `addr2line`):

```text
CPU 0 (RFLAGS IF=0, spinning)            CPU 1 (halted, idle)
spin::Mutex::lock  <- KernelHeap::lock_tracked
<KernelHeap as GlobalAlloc>::alloc
VecDeque::push_back -> grow
PriorityRoundRobin::enqueue  <- PerCpuScheduler::enqueue
sched::try_wake  <- sleep_ns_interruptible_as::wake_callback
hrtimer::process_expired  <- handle_timer_irq  <- isr_timer
   --- the timer interrupt, taken here: ---
heap::check_poison  <- HeapInner::slab_alloc   (holding the heap lock)
<KernelHeap as GlobalAlloc>::alloc  <- Box::default
proc::signal::start_spawned  <- spawn::start_job_and_signals
spawn::spawn_process  <- spawn::self_test_native_device_door
```

The wake chose the idle second CPU (§1558), and that CPU's run queue at the
task's level had never held a task, so its `VecDeque` had no room and grew.
On one CPU every queue has long since grown, which is why only two-CPU boots
hung.

**The decision:**

1. **`KernelHeap::lock_tracked` turns interrupts off for the whole hold**
   (interrupts off, then preemption, then the spinlock; released in the
   reverse order: lock, interrupts back as they were, preemption). An
   interrupt's allocation can then only wait for another CPU's holder, which
   finishes. Proven by `heap::self_test_lock_irqs`, run once interrupts are
   on; with the change taken out it fails ("the heap lock was held with
   interrupts on").
2. **Nothing under the heap lock waits for another CPU.** A slab class that
   runs out is refilled by `KernelHeap::refill_class`: a frame from the frame
   allocator with the lock *not* held, then carved into slots under it
   (`HeapInner::install_slab`). Large blocks go to and from the buddy
   allocator with no heap lock at all (`HeapInner::large_alloc`/
   `large_dealloc`, which need only the direct map's offset, now an atomic).
   The frame allocator's slow path reclaims, compacts and kills; reclaim
   swaps pages out of programs, which shoots down TLBs on every CPU. Under a
   lock held with interrupts off, a second CPU spinning on that lock, also
   with interrupts off, could never answer the shootdown. Before this change
   the refill ran under the lock too, which was a latent deadlock of its own:
   reclaim that allocated from the heap would have spun on the lock its own
   CPU held.

| | For | Against |
|---|---|---|
| **Interrupts off under the heap lock; frames fetched outside it (chosen; Linux's `spin_lock_irqsave` and SLUB's `new_slab`)** | every allocation made in interrupt context is safe -- there is more than the one found: the per-CPU slab cache already fell through to this lock "when an ISR interrupted us mid-operation", by design; one fix for the whole class | interrupts wait for the length of a heap critical section (a free-list pop, or a debug build's poison check of one slot) |
| Forbid allocation in interrupt context and make the run queues allocation-free | no interrupt latency added; an enqueue cannot fail for want of memory | every backend's queues (round-robin `VecDeque`s, EEVDF's and the deadline backend's trees) rebuilt; and every other allocation an interrupt makes found and removed, with nothing to catch the next one -- it is still worth doing for the queues, see below |
| Leave interrupts on and spin with `try_lock` in interrupt context | nothing changes outside interrupts | an allocation in interrupt context fails whenever the lock is held: a lost wake |

**Left, recorded in known-issues:** `A-SCHEDULER-RUN-QUEUES-ALLOCATE-ON-ENQUEUE`
-- a wake can still allocate (now safely), and an allocation that fails for
want of memory there would panic the kernel.

**How to reverse:** take the interrupt save/restore out of `lock_tracked` and
`TrackedGuard::drop`; the refill and large paths can stay outside the lock
either way, and should.
