## 269. hrtimer uses a binary min-heap over a slot slab, not a bare `BinaryHeap` with lazy cancellation

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Each CPU's list of pending timers ("wake me in 3 milliseconds")
used to be kept in deadline order in a plain array, so inserting a timer or
removing one meant shuffling every later entry along — and that shuffle happened
with the CPU's interrupts switched off, where it delays everything on the
machine. It is now a *heap*: a tree kept in an array, where the earliest
deadline is always at the top and any single change touches only about a dozen
entries instead of thousands. The choice recorded here is not "use a heap" —
that part is obvious — but *which* heap, because the obvious one has a defect
that shows up only under a workload that cancels timers more often than it lets
them fire.

**The obvious design, and why it was rejected.** `todo.txt` had sketched the
textbook answer: a `BinaryHeap<Reverse<TimerEntry>>` plus **lazy cancellation**
(cancel just records the id in a set of dead ids; when an entry is popped, look
it up and skip it if it is dead). The reasoning was that a heap has no cheap
remove-by-id, which is true of a *bare* heap: to remove a specific element you
must first find it, and finding it is a linear scan — the very cost the heap was
meant to remove.

Lazy cancellation trades that away for an unbounded one. A cancelled timer stays
in the heap until its deadline arrives, so a workload that arms a long timeout
and cancels it early — which is what *every* successful I/O operation with a
timeout does — accumulates dead entries at exactly the rate it succeeds. Under
that load the heap grows without bound, the "n" in O(log n) is the count of
mostly-dead entries, and the `MAX_TIMERS_HARD_CEILING` that exists to refuse
rather than to silently evict starts refusing *live* timers because dead ones
filled the queue. The failure is workload-shaped and invisible on a quiet boot,
which is the worst combination.

**What was built instead.** Timers live in a slab of slots with a free list. The
heap holds `{expiry_ns, id, slot}` nodes, and each slot stores **its own current
heap position**, maintained by the same swap that every sift already performs.
So the premise "a heap has no cheap remove-by-id" is simply not true of a heap
whose elements know where they are: cancel is an O(1) index into the slab
followed by an O(log n) sift. No tombstones, no dead-id set, no growth.

| | bare heap + lazy cancel | heap + slot slab (chosen) |
|---|---|---|
| cancel cost | O(1) to mark | O(1) index + O(log n) sift |
| dead entries | grow with the cancel rate | none, ever |
| worst-case `n` | armed + all cancelled-not-yet-expired | armed only |
| extra memory | a set of dead ids | one `u32` per slot |
| failure mode | ceiling refuses live timers under a cancel-heavy load | — |

The cost is that the slab must be kept honest, which is where the next two
decisions come from.

**Handles carry `(slot, generation)`, and freeing a slot bumps the generation.**
A direct index is only safe if a stale handle cannot use it. Without the
generation, cancelling through a handle whose timer had already fired would
evict whichever timer was later given the recycled slot — a live timer
destroyed, its waiter left waiting forever. That is not a hypothetical: it is
`BUG-HRTIMER-EVICTS-AN-ARMED-TIMER` from earlier the same week, arriving by a
different route. Lazy cancellation would not have had this exposure, and that is
the one genuine advantage it holds; a 32-bit generation per slot closes it for a
`u32` of memory and one comparison. The handle's id is compared too, as defence
in depth.

**The ordering key is `(expiry_ns, id)`, not `expiry_ns`.** A binary heap is
**not stable**: among elements that compare equal, which one comes out first is
whatever the sifting happened to do. The sorted array it replaced *was* stable,
so this is a behaviour that would have been silently lost. It matters because
equal deadlines are common — millisecond-granularity timeouts armed in a burst
land on the same nanosecond far more often than intuition suggests — and an
unstable heap can pass over one such timer indefinitely as new ones sharing its
deadline arrive. Ids are monotonic, so ordering on them second restores FIFO
among equal deadlines at the cost of one extra comparison taken only on a tie.

**`heap_remove_at` sifts both ways, and this is the subtle one.** Removing an
interior element moves the tail into the hole. The usual case needs a sift-down.
But the tail came from a *different branch* of the tree, so it can be smaller
than the hole's parent and need a sift-**up** instead. Doing only the sift-down
leaves a structure that still looks like a heap and still pops *a* minimum — just
not always the right one, and only when a cancel happened to hit the wrong
branch. Test 8 exists specifically for this: it arms nine timers in a shuffled
order, cancels four interior ranks, and requires the five survivors in deadline
order.

**Not chosen: a timer wheel.** Linux uses a hashed timer wheel for coarse
timeouts and a red-black tree for hrtimers, and the wheel gives O(1) rather than
O(log n) insertion. It was rejected because the wheel's O(1) is amortised over a
cascade step whose cost is bursty and lands in the timer ISR, and because a
wheel quantises deadlines to its bucket width — which is precisely the property
a *high-resolution* timer must not have. The rbtree half of Linux's design is
the closer analogue, and a heap is that with a better constant factor and no
allocation per node.

**Not chosen: sizing both `Vec`s statically to the hard ceiling.** 16 CPUs x
4096 slots is megabytes permanently resident to serve a queue that normally
holds under 64 entries. They grow on demand instead; the ceiling still refuses
beyond 4096, which is what bounds the memory.
