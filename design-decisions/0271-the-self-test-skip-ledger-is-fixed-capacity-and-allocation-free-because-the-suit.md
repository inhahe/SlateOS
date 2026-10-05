## 271. The self-test skip ledger is fixed-capacity and allocation-free, because the suite that most needs it runs before the heap

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** SlateOS's self-tests can decline to run a section — "no AC97
device", "/tmp isn't mounted" — and §270 added a small bookkeeping object, the
*skip ledger*, so the suite's final line says how many sections were skipped
instead of a bare "PASSED". That object stored its notes in a growable list,
which means asking the kernel's memory allocator for space. One of its users,
the physical-memory self-test, runs *before the allocator exists*. The very
first note it tried to file killed the boot. The ledger is now a fixed-size
array that never asks for memory, and a new test proves it, by watching the
allocator's own counters while the ledger is exercised.

### What happened

Boot test batch39 died 120 lines in:

```
[mm]   Double-free detection: OK
!!! KERNEL PANIC !!!
panicked at library/alloc/src/alloc.rs:553:9:
memory allocation of 128 bytes failed
  Heap: slab=0/0 allocs/frees, large=0/0, refills=0, failures=0
```

The panic report is self-explaining if you read the last line: the heap has
served **zero** allocations, ever. `mm::frame::self_test()` runs between
`[mm] Physical frame allocator initialized` and `[mm] Kernel heap allocator
initialized`. The §270 sweep had replaced its `SKIP (HHDM not ready)` print
with `skips.record(…)`, and `record` was `Vec::push` — the first `Vec::push`
of the boot, against an allocator that does not yet exist.

The line it replaced had printed fine for months, because a `serial_println!`
of a `&'static str` allocates nothing.

### The decision

Make the ledger structurally incapable of allocating: entries live in an
inline `[(&'static str, &'static str); 16]`, and `suffix()` returns a
`Display` adaptor instead of a `String`.

The type's own doc comment had **already made this argument** and then failed
to act on it — it justified `&'static str` entries on the grounds that "a skip
reason is a property of the code, not of the run, so there is … no allocation
to fail in a suite that may be diagnosing the allocator", while the `Vec`
holding them allocated on every push. That is the interesting part of this
entry: the invariant was known, written down, and violated by the same
declaration that stated it. A comment is not an enforcement mechanism.

### Alternatives considered

| Option | Why not |
|---|---|
| Leave `Vec`; have `mm::frame` keep printing raw `SKIP` lines | Re-opens §270's failure #1 for the *one* suite where a wrong answer is worst — the memory subsystem. A skip that does not reach the summary gets believed. |
| Leave `Vec`; make `record` fall back to printing when the heap is down | Needs a "is the heap up?" predicate on a path that must not be wrong, and makes the *reporting* behaviour depend on boot phase — so the same skip renders two different ways and only one of them reaches the count. |
| `heapless::Vec` or a similar crate | An inline array plus a length is the whole of what that would buy, in a type with three methods. Not worth a kernel dependency. |
| Fixed capacity (chosen) | Bounded, `const`-constructible, 256 bytes on a stack that has room for it. Costs a capacity limit — addressed below. |

### The capacity limit, and why overflow is counted rather than dropped

Sixteen is generous: the largest suite in the tree records six. But a cap that
silently drops the 17th skip would reintroduce **exactly** the silence §270
exists to prevent, in the least visible way possible — the suite would still
print a number, and the number would be wrong.

So overflow is counted, not dropped. `record` past capacity increments a
separate counter; `report` prints `SKIP: N further section(s) (ledger holds
16)`; and `suffix()` counts named and unnamed together, so the closing line's
total is right even when the ledger cannot name every entry. Degrading from
"named" to "counted" is acceptable. Degrading to "silent" is not.

### Why it is pinned by a runtime test rather than left to structure

The invariant is structural, and structure is one convenient field away from
regressing — the `Vec` version was itself structurally obvious to whoever wrote
it. What makes this worth a test is the *failure mode*: a panic inside
`alloc.rs`, in a suite whose name does not appear in the backtrace, 120 lines
into a boot, with the cause (`slab=0/0`) visible only to a reader who already
suspects it. Nothing about that panic points at this module.

So `fs::selftest::self_test()` snapshots `mm::heap::stats()`, exercises
`record`/`report`/`suffix` — including the overflow arm and `report`'s serial
output, because a reporting path that allocates fails precisely when there is
something to report — and asserts the slab and large-allocation counters did
not move. It runs from `main.rs` immediately after `heap::init`, which is the
earliest point in boot where `heap::stats()` could observe an allocation at
all; the code it guards runs earlier still.

Interrupts are masked across the measurement, because `heap::stats()`
aggregates per-CPU counters and an interrupt handler that allocated inside the
window would be attributed to this code and fail the test spuriously.

### The general lesson

§270's rule was "a self-test may skip, but it must skip for a reason it looked
up, and it must say so in the line a reader believes." This adds a corollary
about the machinery that carries the message: **the reporting path of a
self-test must be weaker in its requirements than anything it reports on.** A
ledger that needs the heap cannot report on the heap; a ledger that needs the
frame allocator cannot report on the frame allocator. `&'static str` in an
inline array needs nothing but a stack, which is why it is the right shape here
and not merely a convenient one.
